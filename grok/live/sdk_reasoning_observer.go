package live

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"sync"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

const historyToolIdentityLimit = 4096

type sdkHistoryObservation struct {
	Method, ThreadID, TurnID string
	Turn                     historyTurn
	Item                     historyItem
}

// SDK callbacks own private bounded observations. Only run boundaries copy
// them into public evidence; no token, identifier or raw envelope is exported.
type sdkHistoryObserver struct {
	mu                   sync.Mutex
	probe                historyProbe
	observations         Evidence
	reply                historyReply
	acceptedCalls        map[[sha256.Size]byte]struct{}
	pending              []sdkHistoryObservation
	pendingBytes         int
	active, reconciled   bool
	inlineTerminal       bool
	deliveredTyped       int
	recoveredTyped       int
	callbackTerminalSeen bool
	cause                error
	changed              chan struct{}
	failures             chan error
}

func newSDKHistoryObserver() *sdkHistoryObserver {
	observer := &sdkHistoryObserver{
		reply:         historyReply{identities: make(map[[sha256.Size]byte]historyReplyOutcome)},
		acceptedCalls: make(map[[sha256.Size]byte]struct{}), changed: make(chan struct{}, 1), failures: make(chan error, 1),
	}
	observer.probe.evidence = &observer.observations
	return observer
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

func (observer *sdkHistoryObserver) request(_ context.Context, request protocolv2.ServerRequest) (codexsdk.ServerRequestResponse, error) {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	call, ok := request.AsItemToolCall()
	if !ok {
		cause := observer.reject(failedObservation("unsupported_server_request", "live: unsupported server request"))
		// Known approval requests get a typed denial even if normal fixture
		// shutdown begins before the SDK writes the callback response.
		if _, command := request.AsItemCommandExecutionRequestApproval(); command {
			return codexsdk.CommandExecutionApprovalResponse(protocolv2.CommandExecutionRequestApprovalResponse{
				Decision: protocolv2.NewCommandExecutionApprovalDecisionDecline(),
			}), nil
		}
		if _, file := request.AsItemFileChangeRequestApproval(); file {
			return codexsdk.FileChangeApprovalResponse(protocolv2.FileChangeRequestApprovalResponse{
				Decision: protocolv2.FileChangeApprovalDecisionDecline,
			}), nil
		}
		return codexsdk.ServerRequestResponse{}, cause
	}
	params := call.Params
	arguments, object := params.Arguments.AsObject()
	text, success := "History probe is unavailable during continuation.", false
	valid := observer.active && params.ThreadID == observer.probe.threadID && params.TurnID != "" && params.TurnID != observer.probe.previousID &&
		(observer.probe.turnID == "" || params.TurnID == observer.probe.turnID) && params.CallID != "" &&
		(params.Namespace == nil || params.Namespace.Value == nil) && params.Tool == historyTool && object && len(arguments) == 0
	if !valid || observer.cause != nil {
		observer.reject(failedObservation("history_tool_request_unattributed", "live: history tool request not authorized"))
	} else if observer.probe.previousID != "" {
		observer.probe.turnID = params.TurnID
		observer.observations.DeniedToolCalls++
	} else {
		identity := sha256.Sum256([]byte(params.CallID))
		_, seen := observer.acceptedCalls[identity]
		if !seen && len(observer.acceptedCalls) == historyToolIdentityLimit {
			observer.reject(failedObservation("history_tool_identity_budget_exceeded", "live: history tool identity evidence budget exceeded"))
		} else {
			if observer.probe.token == "" {
				var nonce [32]byte
				if _, err := rand.Read(nonce[:]); err != nil {
					return codexsdk.ServerRequestResponse{}, observer.reject(failedObservation("history_token_unavailable", "live: fresh tool result unavailable"))
				}
				observer.probe.token = hex.EncodeToString(nonce[:])
			}
			observer.probe.turnID = params.TurnID
			observer.acceptedCalls[identity] = struct{}{}
			observer.observations.ToolCalls++
			text, success = observer.probe.token, true
		}
	}
	return codexsdk.DynamicToolResponse(protocolv2.DynamicToolCallResponse{
		ContentItems: []protocolv2.DynamicToolCallOutputContentItem{protocolv2.NewDynamicToolCallOutputContentItemInputText(protocolv2.DynamicToolCallOutputContentItemInputText{Text: text})}, Success: success,
	}), nil
}

func (observer *sdkHistoryObserver) notification(_ context.Context, notification codexsdk.RawNotification) error {
	raw, err := notification.Envelope.MarshalJSON()
	return observer.observeEnvelope(notification.Method, raw, err)
}

func (observer *sdkHistoryObserver) typedNotification(_ context.Context, notification protocolv2.ServerNotification) error {
	method := ""
	if _, completed := notification.AsItemCompleted(); completed {
		method = "item/completed"
	} else if _, terminal := notification.AsTurnCompleted(); terminal {
		method = "turn/completed"
	} else {
		return nil
	}
	// Known lifecycle events use typed SDK delivery. Only the standalone raw
	// reasoning extension requires retaining an additional raw envelope.
	raw, err := json.Marshal(notification)
	return observer.observeEnvelope(method, raw, err)
}

func (observer *sdkHistoryObserver) observeEnvelope(method string, raw []byte, err error) error {
	var envelope struct {
		Params sdkHistoryObservation `json:"params"`
	}
	if err != nil || json.Unmarshal(raw, &envelope) != nil {
		observer.mu.Lock()
		defer observer.mu.Unlock()
		return observer.reject(failedObservation("invalid_history_evidence", "live: invalid history evidence"))
	}
	observation := envelope.Params
	observation.Method = method
	observer.mu.Lock()
	defer observer.mu.Unlock()
	if !observer.active || observation.ThreadID != observer.probe.threadID && observation.Method != "turn/completed" {
		return nil
	}
	if !observer.reconciled {
		if len(observer.pending) == 8192 || observer.pendingBytes+len(raw) > 8<<20 {
			return observer.reject(failedObservation("early_evidence_budget_exceeded", "live: early evidence budget exceeded"))
		}
		observer.pending = append(observer.pending, observation)
		observer.pendingBytes += len(raw)
		return nil
	}
	return observer.acceptCallback(observation)
}

func (observer *sdkHistoryObserver) matchingTyped(observation sdkHistoryObservation) bool {
	return observation.ThreadID == observer.probe.threadID &&
		(observation.Method == "item/completed" && observation.TurnID == observer.probe.turnID ||
			observation.Method == "turn/completed" && observation.Turn.ID == observer.probe.turnID)
}

func (observer *sdkHistoryObserver) acceptCallback(observation sdkHistoryObservation) error {
	if !observer.callbackTerminalSeen {
		matching := observer.matchingTyped(observation)
		if matching {
			observer.deliveredTyped++
		}
		// Even a wrong-thread terminal can stop this pinned SDK's history.
		// It has no proof authority, and later callbacks must not shift the
		// retained matching prefix used during failure-snapshot recovery.
		if observation.Method == "turn/completed" && observation.Turn.ID == observer.probe.turnID {
			observer.callbackTerminalSeen = true
		}
		if matching && observer.deliveredTyped <= observer.recoveredTyped {
			return observer.cause
		}
	}
	return observer.accept(observation)
}

func (observer *sdkHistoryObserver) accept(observation sdkHistoryObservation) error {
	if observation.ThreadID != observer.probe.threadID || observer.cause != nil {
		return observer.cause
	}
	var err error
	if observation.Method == "turn/completed" {
		if observation.Turn.ID != observer.probe.turnID {
			return nil
		}
		observer.correlateTools(observation.Turn.Items)
		err = observer.probe.completedTurn(observation.Turn, &observer.reply)
	} else if observation.TurnID == observer.probe.turnID {
		if observation.Method == codexsdk.RawResponseItemCompletedExtension {
			if observer.probe.previousID == "" && observation.Item.Type == "reasoning" && observation.Item.ID != "" {
				observer.observations.ReasoningItems++
				if observation.Item.EncryptedContent != "" {
					observer.observations.EncryptedItems++
				}
			}
		} else {
			items := []historyItem{observation.Item}
			observer.correlateTools(items)
			err = observer.probe.completedItem(items[0], &observer.reply)
		}
	}
	if err != nil {
		return observer.reject(err)
	}
	if observer.reply.authoritative && !observer.reply.recall {
		return observer.reject(failedObservation("history_reply_not_recalled", "live: final reply did not recall tool result"))
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
	if run.Turn.ID == "" || run.Turn.ID == observer.probe.previousID || observer.probe.turnID != "" && observer.probe.turnID != run.Turn.ID {
		return observer.reject(failedObservation("turn_identity_missing", "live: turn identity not established"))
	}
	observer.probe.turnID, observer.reconciled = run.Turn.ID, true
	observer.observations.Stage = "turn_submitted"
	if observer.probe.previousID != "" {
		observer.observations.Stage = "continuation_submitted"
	}
	for _, observation := range observer.pending {
		_ = observer.acceptCallback(observation)
	}
	observer.pending, observer.pendingBytes = nil, 0
	hasTerminal := false
	for _, notification := range run.Notifications {
		if _, terminal := notification.AsTurnCompleted(); terminal {
			hasTerminal = true
		}
	}
	if !hasTerminal && run.Turn.Status != protocolv2.TurnStatusInProgress {
		observer.inlineTerminal = true
		raw, err := json.Marshal(run.Turn)
		var terminal historyTurn
		if err != nil || json.Unmarshal(raw, &terminal) != nil {
			return observer.reject(failedObservation("invalid_history_evidence", "live: invalid history evidence"))
		}
		_ = observer.accept(sdkHistoryObservation{Method: "turn/completed", ThreadID: observer.probe.threadID, Turn: terminal})
	}
	return observer.cause
}

func (observer *sdkHistoryObserver) recoverNotifications(notifications []protocolv2.ServerNotification) {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	// A provisional request identity cannot replace a missing or mismatched
	// turn/start response, including during SDK EOF/failure snapshot recovery.
	if !observer.reconciled {
		return
	}
	matchingPrefix := 0
	for _, notification := range notifications {
		raw, err := json.Marshal(notification)
		var envelope struct {
			Method string                `json:"method"`
			Params sdkHistoryObservation `json:"params"`
		}
		if err == nil && json.Unmarshal(raw, &envelope) == nil {
			envelope.Params.Method = envelope.Method
			if !observer.matchingTyped(envelope.Params) {
				continue
			}
			matchingPrefix++
			if matchingPrefix > observer.deliveredTyped && matchingPrefix > observer.recoveredTyped {
				observer.recoveredTyped = matchingPrefix
				_ = observer.accept(envelope.Params)
			}
		}
	}
}

func (observer *sdkHistoryObserver) finishTurn() {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	observer.active = false
	recall := observer.reply.recall && !observer.reply.ambiguous
	if observer.probe.previousID == "" {
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
