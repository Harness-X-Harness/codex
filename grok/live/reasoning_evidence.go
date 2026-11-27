package live

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
	"strings"
	"sync"
)

const historyToolIdentityLimit = 4096

type sdkHistoryObservation struct {
	Method, ThreadID, TurnID string
	Turn                     historyTurn
	Item                     historyItem
}

// SDK callbacks own bounded observations. Run boundaries copy only proof facts
// into the safe summary; optional diagnostics retain public task results, never
// private reasoning or raw envelopes.
type sdkHistoryObserver struct {
	mu                           sync.Mutex
	diagnostics                  *Diagnostics
	threadID, turnID, previousID string
	unitPrice                    int
	toolResult                   string
	observations                 Evidence
	reply                        historyReply
	acceptedCalls                map[[sha256.Size]byte]struct{}
	pending                      []sdkHistoryObservation
	pendingBytes                 int
	active, reconciled           bool
	inlineTerminal               bool
	deliveredTyped               int
	recoveredTyped               int
	callbackTerminalSeen         bool
	cause                        error
	changed                      chan struct{}
	failures                     chan error
}

func newSDKHistoryObserver() *sdkHistoryObserver {
	observer := &sdkHistoryObserver{
		reply:         historyReply{identities: make(map[[sha256.Size]byte]historyReplyOutcome)},
		acceptedCalls: make(map[[sha256.Size]byte]struct{}), changed: make(chan struct{}, 1), failures: make(chan error, 1),
	}
	return observer
}

// beginContinuation owns the per-turn proof reset. The same observer retains
// prior-turn contradiction checks and the tool price, not prior reply credit.
func (observer *sdkHistoryObserver) beginContinuation() string {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	observer.observations.Stage = "first_turn_proven"
	threadID := observer.threadID
	observer.previousID, observer.turnID = observer.turnID, ""
	observer.reconciled = false
	observer.inlineTerminal = false
	observer.deliveredTyped, observer.recoveredTyped = 0, 0
	observer.callbackTerminalSeen = false
	observer.reply = historyReply{identities: make(map[[sha256.Size]byte]historyReplyOutcome)}
	return threadID
}

func (observer *sdkHistoryObserver) reject(err error) error {
	if observer.cause == nil {
		observer.cause = err
		observer.failures <- err
	}
	observer.signal()
	return observer.cause
}

func (observer *sdkHistoryObserver) signal() {
	select {
	case observer.changed <- struct{}{}:
	default:
	}
}

func (observer *sdkHistoryObserver) failure(cause error) error {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	if observer.cause != nil {
		return observer.cause
	}
	return sdkFailure(cause)
}

func (observer *sdkHistoryObserver) notification(_ context.Context, notification codexsdk.RawNotification) error {
	raw, err := notification.Envelope.MarshalJSON()
	if err == nil && notification.Method == codexsdk.RawResponseItemCompletedExtension {
		recordResponseToolEnvelopeDiagnostic(observer.diagnostics, "reasoning_response_item", raw)
	}
	return observer.observeEnvelope(notification.Method, raw, err)
}

func (observer *sdkHistoryObserver) typedNotification(_ context.Context, notification protocolv2.ServerNotification) error {
	observation, relevant, err := historyObservationFromSDK(notification)
	if !relevant {
		return nil
	}
	// The early-evidence limit counts the entire encoded SDK notification,
	// including fields outside the projection. Serialization measures that exact
	// byte budget only; typed fields are the authority for the observation.
	raw, marshalErr := json.Marshal(notification)
	if err == nil {
		err = marshalErr
	}
	return observer.observe(observation, len(raw), err)
}

// Only the raw response-item extension crosses a dynamic JSON boundary.
func (observer *sdkHistoryObserver) observeEnvelope(method string, raw []byte, err error) error {
	var envelope struct {
		Params sdkHistoryObservation `json:"params"`
	}
	if err == nil {
		err = json.Unmarshal(raw, &envelope)
	}
	envelope.Params.Method = method
	return observer.observe(envelope.Params, len(raw), err)
}

func (observer *sdkHistoryObserver) observe(observation sdkHistoryObservation, size int, err error) error {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	if err != nil {
		return observer.reject(failedObservation("invalid_history_evidence", "live: invalid history evidence"))
	}
	// Finished/current-prior turns retain contradiction checks through callback
	// join, but late delivery cannot add positive completion or replay credit.
	prior := observer.previousID != "" && (observation.TurnID == observer.previousID || observation.Turn.ID == observer.previousID)
	if observation.ThreadID == observer.threadID && (!observer.active || prior) {
		if observation.Method == "item/completed" && (observation.TurnID == observer.turnID || prior) && observation.Item.Type == "agentMessage" && observation.Item.ID == "" {
			return observer.reject(failedObservation("assistant_identity_missing", "live: assistant item identity not established"))
		}
		if observation.Method == "turn/completed" && (observation.Turn.ID == observer.turnID || prior) {
			if observation.Turn.Status != "completed" || len(observation.Turn.Error) != 0 && string(observation.Turn.Error) != "null" {
				return observer.reject(failedTurn(observation.Turn.Status, observation.Turn.Error, "live: turn did not complete successfully"))
			}
			for _, item := range observation.Turn.Items {
				if item.Type == "agentMessage" && item.ID == "" {
					return observer.reject(failedObservation("assistant_identity_missing", "live: assistant item identity not established"))
				}
			}
		}
		return observer.cause
	}
	if !observer.active || observation.ThreadID != observer.threadID && observation.Method != "turn/completed" {
		return nil
	}
	if !observer.reconciled {
		if len(observer.pending) == 8192 || observer.pendingBytes+size > 8<<20 {
			return observer.reject(failedObservation("early_evidence_budget_exceeded", "live: early evidence budget exceeded"))
		}
		observer.pending = append(observer.pending, observation)
		observer.pendingBytes += size
		return nil
	}
	return observer.acceptCallback(observation)
}

func (observer *sdkHistoryObserver) matchingTyped(observation sdkHistoryObservation) bool {
	return observation.ThreadID == observer.threadID &&
		(observation.Method == "item/completed" && observation.TurnID == observer.turnID ||
			observation.Method == "turn/completed" && observation.Turn.ID == observer.turnID)
}

func (observer *sdkHistoryObserver) acceptCallback(observation sdkHistoryObservation) error {
	if !observer.callbackTerminalSeen {
		matching := observer.matchingTyped(observation)
		if matching {
			observer.deliveredTyped++
		}
		// Terminal delivery ends the retained run prefix. Client-wide
		// callbacks may continue; they must not shift recovery accounting.
		if observation.Method == "turn/completed" && observation.Turn.ID == observer.turnID {
			observer.callbackTerminalSeen = true
		}
		if matching && observer.deliveredTyped <= observer.recoveredTyped {
			return observer.cause
		}
	}
	return observer.accept(observation)
}

func (observer *sdkHistoryObserver) accept(observation sdkHistoryObservation) error {
	if observation.ThreadID != observer.threadID || observer.cause != nil {
		return observer.cause
	}
	var err error
	if observation.Method == "turn/completed" {
		if observation.Turn.ID != observer.turnID {
			return nil
		}
		observer.correlateTools(observation.Turn.Items)
		err = observer.completedTurn(observation.Turn, &observer.reply)
	} else if observation.TurnID == observer.turnID {
		if observation.Method == codexsdk.RawResponseItemCompletedExtension {
			if observer.previousID == "" && observation.Item.Type == "reasoning" && observation.Item.ID != "" {
				observer.observations.ReasoningItems++
				if observation.Item.EncryptedContent != "" {
					observer.observations.EncryptedItems++
				}
			}
		} else {
			items := []historyItem{observation.Item}
			observer.correlateTools(items)
			err = observer.completedItem(items[0], &observer.reply)
		}
	}
	if err != nil {
		return observer.reject(err)
	}
	observer.signal()
	return nil
}

func (observer *sdkHistoryObserver) correlateTools(items []historyItem) {
	for index := range items {
		if items[index].Type == "dynamicToolCall" {
			if _, accepted := observer.acceptedCalls[sha256.Sum256([]byte(items[index].ID))]; !accepted {
				items[index].Success = false
			}
		}
	}
}

func (observer *sdkHistoryObserver) reconcile(run codexsdk.ThreadRunResult) error {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	if run.Turn.ID == "" || run.Turn.ID == observer.previousID || observer.turnID != "" && observer.turnID != run.Turn.ID {
		return observer.reject(failedObservation("turn_identity_missing", "live: turn identity not established"))
	}
	observer.turnID, observer.reconciled = run.Turn.ID, true
	observer.observations.Stage = "turn_submitted"
	if observer.previousID != "" {
		observer.observations.Stage = "continuation_submitted"
	}
	for _, observation := range observer.pending {
		_ = observer.acceptCallback(observation)
	}
	observer.pending, observer.pendingBytes = nil, 0
	// The typed retained prefix can be admitted before its callback executes.
	// Consume it under this lock before an inline terminal can grant readiness.
	observer.recoverNotificationsLocked(run.Notifications)
	hasTerminal := false
	for _, notification := range run.Notifications {
		if _, terminal := notification.AsTurnCompleted(); terminal {
			hasTerminal = true
		}
	}
	if !hasTerminal && run.Turn.Status != protocolv2.TurnStatusInProgress {
		observer.inlineTerminal = true
		terminal, err := historyTurnFromSDK(run.Turn)
		if err != nil {
			return observer.reject(failedObservation("invalid_history_evidence", "live: invalid history evidence"))
		}
		_ = observer.accept(sdkHistoryObservation{Method: "turn/completed", ThreadID: observer.threadID, Turn: terminal})
	}
	return observer.cause
}

func (observer *sdkHistoryObserver) recoverNotifications(notifications []protocolv2.ServerNotification) {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	observer.recoverNotificationsLocked(notifications)
}

func (observer *sdkHistoryObserver) recoverNotificationsLocked(notifications []protocolv2.ServerNotification) {
	// A provisional request identity cannot replace a missing or mismatched
	// turn/start response, including during SDK EOF/failure snapshot recovery.
	if !observer.reconciled {
		return
	}
	matchingPrefix := 0
	for _, notification := range notifications {
		observation, relevant, err := historyObservationFromSDK(notification)
		if err != nil || !relevant || !observer.matchingTyped(observation) {
			continue
		}
		matchingPrefix++
		if matchingPrefix > observer.deliveredTyped && matchingPrefix > observer.recoveredTyped {
			observer.recoveredTyped = matchingPrefix
			_ = observer.accept(observation)
		}
	}
}

func (observer *sdkHistoryObserver) finishTurnLocked() {
	observer.active = false
	recall := observer.reply.recall && !observer.reply.ambiguous
	observer.diagnostics.Record("oracle_result", map[string]any{"scenario": "reasoning", "turn_id": observer.turnID, "completed": observer.reply.completed, "recall": recall, "ambiguous": observer.reply.ambiguous, "reply_bytes": observer.reply.bytes, "authoritative": observer.reply.authoritative, "explicit_final": observer.reply.explicitFinal, "reply": observer.reply.text.String(), "format_valid": observer.reply.formatValid})
	if observer.previousID == "" {
		observer.observations.FirstCompleted, observer.observations.FirstRecall, observer.observations.FirstReplyBytes = observer.reply.completed, recall, observer.reply.bytes
		if observer.reply.completed {
			observer.observations.Stage = "first_turn_completed"
		}
	} else {
		observer.observations.Completed, observer.observations.Recalled, observer.observations.ReplyBytes = observer.reply.completed, recall, observer.reply.bytes
		if observer.reply.completed {
			observer.observations.Stage = "continuation_completed"
		}
	}
}

type historyItem struct {
	ID               string  `json:"id"`
	Type             string  `json:"type"`
	Phase            string  `json:"phase"`
	Text             string  `json:"text"`
	Namespace        *string `json:"namespace"`
	Tool             string  `json:"tool"`
	Status           string  `json:"status"`
	Success          bool    `json:"success"`
	ContentItems     []item  `json:"contentItems"`
	EncryptedContent string  `json:"encrypted_content"`
}

type historyTurn struct {
	ID     string          `json:"id"`
	Status string          `json:"status"`
	Error  json.RawMessage `json:"error"`
	Items  []historyItem   `json:"items"`
}

type historyReply struct {
	bytes                                       int
	text                                        strings.Builder
	formatValid, evaluated                      bool
	completed, recall, authoritative, ambiguous bool
	amount                                      *float64
	explicitFinal                               bool
	identities                                  map[[sha256.Size]byte]historyReplyOutcome
}

type historyReplyOutcome struct {
	finalEligible, explicitFinal, hasContent bool
	textSHA                                  [sha256.Size]byte
}

func (observer *sdkHistoryObserver) completedTurn(turn historyTurn, reply *historyReply) error {
	if turn.Status != "completed" || len(turn.Error) != 0 && string(turn.Error) != "null" {
		return failedTurn(turn.Status, turn.Error, "live: turn did not complete successfully")
	}
	reply.completed = true
	// The public MessagePhase owner distinguishes terminal answers from
	// commentary/partial text. An authoritative summary combines all explicit
	// final answers, with phase-unknown legacy fallback only when no final exists.
	summaryAssistant := false
	summaryIdentities := make(map[[sha256.Size]byte]historyReplyOutcome)
	for _, item := range turn.Items {
		if item.Type == "agentMessage" {
			if item.ID == "" {
				return failedObservation("assistant_identity_missing", "live: assistant item identity not established")
			}
			if !summaryAssistant {
				// A nonempty assistant summary supersedes notifications even when
				// it has no eligible result; it cannot borrow an older answer.
				observer.diagnostics.Record("oracle_reply_reset", map[string]any{"scenario": "reasoning", "turn_id": observer.turnID, "source": "terminal_items"})
				reply.bytes, reply.recall, reply.explicitFinal = 0, false, false
				reply.text.Reset()
				reply.formatValid, reply.evaluated = false, false
				reply.authoritative, reply.ambiguous = true, false
				reply.identities = nil
				summaryAssistant = true
			}
			identity := sha256.Sum256([]byte(item.ID))
			outcome := historyItemOutcome(item)
			if previous, seen := summaryIdentities[identity]; seen {
				if previous != outcome {
					reply.ambiguous = true
					return failedObservation("assistant_identity_conflict", "live: authoritative reply identity conflicted")
				}
				continue
			}
			if len(summaryIdentities) == historyAssistantLimit {
				return failedObservation("assistant_identity_budget_exceeded", "live: assistant identity evidence budget exceeded")
			}
			summaryIdentities[identity] = outcome
			if strings.TrimSpace(item.Text) == "" {
				continue
			}
			if item.Phase == "final_answer" || item.Phase == "" && !reply.explicitFinal {
				observer.addReplyFragment(item, reply)
			}
		} else if err := observer.completedItem(item, reply); err != nil {
			return err
		}
	}
	observer.evaluateReply(reply)
	return nil
}

func (observer *sdkHistoryObserver) completedItem(item historyItem, reply *historyReply) error {
	if item.Type == "agentMessage" {
		if item.ID == "" {
			return failedObservation("assistant_identity_missing", "live: assistant item identity not established")
		}
		if reply.authoritative {
			return nil
		}
		identity := sha256.Sum256([]byte(item.ID))
		outcome := historyItemOutcome(item)
		if previous, seen := reply.identities[identity]; seen {
			if previous != outcome {
				reply.ambiguous = true
			}
			return nil
		}
		if len(reply.identities) == historyAssistantLimit {
			return failedObservation("assistant_identity_budget_exceeded", "live: assistant identity evidence budget exceeded")
		}
		reply.identities[identity] = outcome
		// Empty finalized text contributes no candidate, but still carries identity.
		if !outcome.hasContent {
			return nil
		}
		// Combine unseen eligible fragments from the entire final response.
		// Explicit finals outrank legacy text; commentary never earns recall.
		// Replay and conflicting identities stay above.
		if item.Phase == "final_answer" || item.Phase == "" && !reply.explicitFinal {
			observer.addReplyFragment(item, reply)
		}
	}
	if observer.previousID != "" {
		return nil
	}
	if item.Type == "reasoning" {
		observer.observations.ReasoningItems++
	}
	if item.Type == "dynamicToolCall" && item.ID != "" && item.Namespace == nil && item.Tool == historyTool && item.Status == "completed" && item.Success && observer.unitPrice > 0 && observer.toolResult != "" {
		for _, content := range item.ContentItems {
			if content.Type == "inputText" && strings.Contains(content.Text, observer.toolResult) {
				observer.observations.CompletedTools++
				break
			}
		}
	}
	return nil
}

// All eligible final fragments form one structured result; never pick a valid
// object from a larger malformed answer. Final phase still outranks legacy text.
func (observer *sdkHistoryObserver) addReplyFragment(item historyItem, reply *historyReply) {
	observer.diagnostics.Record("oracle_reply_fragment", map[string]any{"scenario": "reasoning", "turn_id": observer.turnID, "item_id": item.ID, "phase": item.Phase, "text": item.Text, "reset_legacy": item.Phase == "final_answer" && !reply.explicitFinal, "authoritative": reply.authoritative})
	if item.Phase == "final_answer" && !reply.explicitFinal {
		reply.bytes = 0
		reply.text.Reset()
	}
	reply.bytes += len(item.Text)
	if reply.text.Len() != 0 {
		reply.text.WriteByte('\n')
	}
	reply.text.WriteString(item.Text)
	reply.formatValid, reply.recall, reply.evaluated = false, false, false
	reply.explicitFinal = item.Phase == "final_answer"
}

// SDK retention and frame limits own resource admission. Assemble the complete
// selected answer without repeatedly copying or parsing its growing prefixes.
// Turn completion and final observation boundaries request evaluation; an
// unchanged collected reply reuses it, while new fragments cannot borrow recall.
func (observer *sdkHistoryObserver) evaluateReply(reply *historyReply) {
	if reply.evaluated {
		return
	}
	var result struct {
		Amount *float64 `json:"amount"`
	}
	err := parseStructuredReply(reply.text.String(), &result, "amount")
	reply.formatValid = err == nil
	reply.amount = result.Amount
	reply.evaluated = true
}

func historyItemOutcome(item historyItem) historyReplyOutcome {
	return historyReplyOutcome{finalEligible: item.Phase == "" || item.Phase == "final_answer", explicitFinal: item.Phase == "final_answer", hasContent: strings.TrimSpace(item.Text) != "", textSHA: sha256.Sum256([]byte(item.Text))}
}

func historyObservationFromSDK(notification protocolv2.ServerNotification) (sdkHistoryObservation, bool, error) {
	if completed, ok := notification.AsItemCompleted(); ok {
		params := completed.Params
		return sdkHistoryObservation{Method: "item/completed", ThreadID: params.ThreadID, TurnID: params.TurnID, Item: historyItemFromSDK(params.Item)}, true, nil
	}
	if completed, ok := notification.AsTurnCompleted(); ok {
		turn, err := historyTurnFromSDK(completed.Params.Turn)
		return sdkHistoryObservation{Method: "turn/completed", ThreadID: completed.Params.ThreadID, Turn: turn}, true, err
	}
	return sdkHistoryObservation{}, false, nil
}

func historyTurnFromSDK(value protocolv2.Turn) (historyTurn, error) {
	result := historyTurn{ID: value.ID, Status: string(value.Status)}
	if value.Error != nil && value.Error.Value != nil {
		// The shared bounded failure classifier consumes the error union as JSON.
		// Lifecycle identity, status and items come directly from the SDK.
		var err error
		result.Error, err = json.Marshal(*value.Error.Value)
		if err != nil {
			return result, err
		}
	}
	for _, item := range value.Items {
		result.Items = append(result.Items, historyItemFromSDK(item))
	}
	return result, nil
}

func historyItemFromSDK(value protocolv2.ThreadItem) historyItem {
	result := historyItem{Type: string(value.Kind())}
	if message, ok := value.AsAgentMessage(); ok {
		result.ID, result.Text = message.ID, message.Text
		if message.Phase != nil && message.Phase.Value != nil {
			result.Phase = string(*message.Phase.Value)
		}
	} else if reasoning, ok := value.AsReasoning(); ok {
		result.ID = reasoning.ID
	} else if tool, ok := value.AsDynamicToolCall(); ok {
		result.ID, result.Tool, result.Status = tool.ID, tool.Tool, string(tool.Status)
		if tool.Namespace != nil && tool.Namespace.Value != nil {
			namespace := *tool.Namespace.Value
			result.Namespace = &namespace
		}
		if tool.Success != nil && tool.Success.Value != nil {
			result.Success = *tool.Success.Value
		}
		if tool.ContentItems != nil && tool.ContentItems.Value != nil {
			for _, content := range *tool.ContentItems.Value {
				projected := item{Type: string(content.Kind())}
				if text, ok := content.AsInputText(); ok {
					projected.Text = text.Text
				}
				result.ContentItems = append(result.ContentItems, projected)
			}
		}
	}
	return result
}
