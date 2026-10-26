package live

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"sync"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

const (
	sdkEditObservationLimit = 4096
	sdkEditByteLimit        = 8 << 20
)

type sdkEditObservation struct {
	ThreadID, TurnID string
	Turn             editTurn
	Item             editItem
	method           string
	callback         bool
}

type sdkEditLedger struct {
	probe                            editProbe
	evidence                         StructuredEditEvidence
	observation                      editObservation
	reconciled                       bool
	callbackTerminal, inlineTerminal bool
	seen                             map[[sha256.Size]byte][sha256.Size]byte
	approvals                        map[string]bool
	pending                          []sdkEditObservation
}

// Callback-owned ledgers retain both turns until the SDK joins callback work.
// Snapshots expose only safe business facts, never retained item content.
type sdkEditObserver struct {
	diagnostics  *Diagnostics
	mu           sync.Mutex
	fixture      *editFixture
	threadID     string
	ledgers      []*sdkEditLedger
	count, bytes int
	cause        error
	changed      chan struct{}
	failures     chan error
	clientDone   <-chan struct{}
}

func newSDKEditObserver(fixture *editFixture) *sdkEditObserver {
	return &sdkEditObserver{fixture: fixture, changed: make(chan struct{}, 1), failures: make(chan error, 1)}
}

func (observer *sdkEditObserver) reject(err error) error {
	if observer.cause == nil {
		observer.cause = sdkFailure(err)
		observer.failures <- observer.cause
	}
	observer.signal()
	return observer.cause
}

func (observer *sdkEditObserver) signal() {
	select {
	case observer.changed <- struct{}{}:
	default:
	}
}

func (observer *sdkEditObserver) bindThread(threadID string) error {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	if threadID == "" || observer.threadID != "" {
		return observer.reject(failedObservation("edit_thread_identity_missing", "live: edit thread identity not established"))
	}
	observer.threadID = threadID
	return observer.cause
}

func (observer *sdkEditObserver) beginTurn() error {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	previous := ""
	if len(observer.ledgers) != 0 {
		previous = observer.ledgers[0].probe.turnID
		if len(observer.ledgers) != 1 || !observer.fixture.continuation || !observer.ledgers[0].reconciled || !observer.ledgers[0].observation.completed {
			return observer.reject(failedObservation("invalid_edit_continuation", "live: edit continuation not authorized"))
		}
		if err := observer.ledgers[0].proof(); err != nil {
			return observer.reject(err)
		}
	}
	if observer.threadID == "" || observer.cause != nil {
		return observer.reject(failedObservation("edit_thread_identity_missing", "live: edit thread identity not established"))
	}
	ledger := &sdkEditLedger{seen: make(map[[sha256.Size]byte][sha256.Size]byte), approvals: make(map[string]bool)}
	ledger.probe = editProbe{threadID: observer.threadID, previousID: previous, fixture: observer.fixture, evidence: &ledger.evidence, base: &ledger.evidence.Evidence}
	observer.ledgers = append(observer.ledgers, ledger)
	return nil
}

func (observer *sdkEditObserver) reconcile(turn protocolv2.Turn) error {
	// Keep the same JSON projection as callbacks: editItem serialization also
	// defines byte charges; replay conflicts compare scenario facts below.
	raw, err := json.Marshal(turn)
	var decoded editTurn
	observer.mu.Lock()
	defer observer.mu.Unlock()
	if len(observer.ledgers) == 0 || err != nil || json.Unmarshal(raw, &decoded) != nil {
		return observer.reject(failedObservation("invalid_edit_evidence", "live: invalid edit evidence"))
	}
	ledger := observer.ledgers[len(observer.ledgers)-1]
	if err := observer.budget(len(raw)); err != nil {
		return err
	}
	if ledger.reconciled || decoded.ID == "" || decoded.ID == ledger.probe.previousID {
		return observer.reject(failedObservation("edit_turn_identity_missing", "live: edit turn identity not established"))
	}
	ledger.probe.turnID, ledger.reconciled = decoded.ID, true
	ledger.evidence.Stage = "edit_turn_submitted"
	if ledger.probe.previousID != "" {
		ledger.evidence.Stage = "edit_continuation_submitted"
	}
	pending := ledger.pending
	ledger.pending = nil
	for _, observation := range pending {
		if err := observer.accept(ledger, observation); err != nil {
			return err
		}
	}
	if decoded.Status != "inProgress" {
		ledger.inlineTerminal = true
		return observer.accept(ledger, sdkEditObservation{ThreadID: observer.threadID, Turn: decoded, method: "turn/completed"})
	}
	observer.signal()
	return observer.cause
}

func (observer *sdkEditObserver) budget(size int) error {
	if observer.count == sdkEditObservationLimit || size > sdkEditByteLimit-observer.bytes {
		return observer.reject(failedObservation("edit_evidence_budget_exceeded", "live: edit evidence budget exceeded"))
	}
	observer.count++
	observer.bytes += size
	return observer.cause
}

func (observer *sdkEditObserver) request(ctx context.Context, request protocolv2.ServerRequest) (codexsdk.ServerRequestResponse, error) {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	observer.noteCallback(ctx)
	raw, marshalErr := json.Marshal(request)
	if marshalErr == nil {
		observer.budget(len(raw))
	}
	if approval, ok := request.AsItemFileChangeRequestApproval(); ok {
		// Every file-change request is declined. A positive fixture has no
		// approval step; losing that denial would let a later write look authorized.
		decline := codexsdk.FileChangeApprovalResponse(protocolv2.FileChangeRequestApprovalResponse{Decision: protocolv2.FileChangeApprovalDecisionDecline})
		params := approval.Params
		observer.diagnostics.Record("approval_response", map[string]any{"kind": "file_change", "turn_id": params.TurnID, "item_id": params.ItemID, "decision": "decline"})
		if observer.fixture != nil && !observer.fixture.decline && params.ThreadID == observer.threadID && params.TurnID != "" && params.ItemID != "" {
			// The typed denial still goes out. Returning the error here would
			// replace it with a transport rejection and hide the authorization fact.
			_ = observer.reject(failedObservation("unexpected_edit_approval", "live: unexpected edit approval denied"))
			return decline, nil
		}
		if len(observer.ledgers) != 0 && observer.fixture.decline && params.ThreadID == observer.threadID && params.TurnID != "" && params.ItemID != "" {
			ledger := observer.ledgers[0]
			if len(observer.ledgers) == 1 && observer.cause == nil && marshalErr == nil {
				candidate := sdkEditObservation{ThreadID: params.ThreadID, TurnID: params.TurnID, Item: editItem{ID: params.ItemID}, method: "file_change_approval"}
				if !ledger.reconciled {
					ledger.pending = append(ledger.pending, candidate)
				} else {
					observer.accept(ledger, candidate)
				}
			}
		}
		return decline, nil
	}
	if approval, ok := request.AsItemCommandExecutionRequestApproval(); ok {
		observer.diagnostics.Record("approval_response", map[string]any{"kind": "command", "turn_id": approval.Params.TurnID, "item_id": approval.Params.ItemID, "decision": "decline"})
		return codexsdk.CommandExecutionApprovalResponse(protocolv2.CommandExecutionRequestApprovalResponse{Decision: protocolv2.NewCommandExecutionApprovalDecisionDecline()}), nil
	}
	return codexsdk.ServerRequestResponse{}, failedObservation("unsupported_server_request", "live: unsupported server request")
}

func (observer *sdkEditObserver) typedNotification(ctx context.Context, notification protocolv2.ServerNotification) error {
	observer.mu.Lock()
	observer.noteCallback(ctx)
	observer.mu.Unlock()
	method := "item/completed"
	if _, ok := notification.AsItemCompleted(); !ok {
		if _, ok := notification.AsTurnCompleted(); !ok {
			return nil
		}
		method = "turn/completed"
	}
	// Charge the full projection before comparing the relevant scenario facts.
	// Its JSON type checks also precede tool checks.
	raw, err := json.Marshal(notification)
	return observer.observeEnvelope(method, raw, err)
}

func (observer *sdkEditObserver) observeEnvelope(method string, raw []byte, marshalErr error) error {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	if err := observer.budget(len(raw)); err != nil {
		return err
	}
	var envelope struct {
		Params sdkEditObservation `json:"params"`
	}
	if marshalErr != nil || json.Unmarshal(raw, &envelope) != nil || len(observer.ledgers) == 0 {
		return observer.reject(failedObservation("invalid_edit_evidence", "live: invalid edit evidence"))
	}
	observation := envelope.Params
	observation.method, observation.callback = method, true
	if observation.ThreadID != observer.threadID {
		return observer.reject(failedObservation("unrelated_edit_thread", "live: unrelated edit thread evidence"))
	}
	id := observation.TurnID
	if method == "turn/completed" {
		id = observation.Turn.ID
	}
	for _, ledger := range observer.ledgers {
		if ledger.reconciled && ledger.probe.turnID == id {
			return observer.accept(ledger, observation)
		}
	}
	ledger := observer.ledgers[len(observer.ledgers)-1]
	if !ledger.reconciled {
		ledger.pending = append(ledger.pending, observation)
		return nil
	}
	return observer.reject(failedObservation("unrelated_edit_turn", "live: unrelated edit turn evidence"))
}

func (observer *sdkEditObserver) accept(ledger *sdkEditLedger, observation sdkEditObservation) error {
	if observer.cause != nil {
		return observer.cause
	}
	if observation.method == "file_change_approval" {
		// A typed decline grants no authority. Only the actual start identity
		// selects matching candidates; unrelated denials cannot poison proof.
		if observation.TurnID != ledger.probe.turnID {
			return nil
		}
		if !ledger.approvals[observation.Item.ID] {
			ledger.approvals[observation.Item.ID] = true
			ledger.evidence.Approvals++
		}
		observer.signal()
		return nil
	}
	if observation.method == "turn/completed" {
		if observation.Turn.ID != ledger.probe.turnID {
			return observer.reject(failedObservation("unrelated_edit_turn", "live: unrelated edit turn evidence"))
		}
		// Validate every terminal, including settled reads and late first-turn
		// contradictions; replay each item through the same deduplication ledger.
		turn := observation.Turn
		items := turn.Items
		turn.Items = nil
		if err := ledger.probe.terminal(turn, &ledger.observation); err != nil {
			return observer.reject(err)
		}
		ledger.callbackTerminal = ledger.callbackTerminal || observation.callback
		for _, item := range items {
			if err := observer.accept(ledger, sdkEditObservation{ThreadID: observation.ThreadID, TurnID: turn.ID, Item: item, method: "item/completed"}); err != nil {
				return err
			}
		}
	} else {
		if observation.TurnID != ledger.probe.turnID {
			return observer.reject(failedObservation("unrelated_edit_turn", "live: unrelated edit turn evidence"))
		}
		item := observation.Item
		raw, _ := json.Marshal(item)
		if err := observer.budget(len(raw)); err != nil {
			return err
		}
		// Tool choice, arguments and intermediate activity do not decide a
		// filesystem outcome. Only final replies and unauthorized committed
		// changes need observation; file-change counts are diagnostic.
		if item.Type != "agentMessage" && item.Type != "fileChange" {
			return nil
		}
		identity := item.ID
		key := sha256.Sum256([]byte(observation.method + "\x00" + item.Type + "\x00" + identity))
		facts, _ := json.Marshal(item)
		digest := sha256.Sum256(facts)
		if previous, exists := ledger.seen[key]; exists {
			if previous != digest {
				return observer.reject(failedObservation("conflicting_edit_item", "live: conflicting edit item evidence"))
			}
			return nil
		}
		if err := ledger.probe.publicItem(item, &ledger.observation); err != nil {
			return observer.reject(err)
		}
		ledger.seen[key] = digest
	}
	observer.signal()
	return nil
}

// A settled read validates both retained terminals; it is not a callback fence.
func (observer *sdkEditObserver) validateSettled(threadID string, turns []protocolv2.Turn) error {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	if threadID != observer.threadID || len(turns) != len(observer.ledgers) || len(turns) == 0 {
		return observer.reject(failedObservation("invalid_settled_edit_thread", "live: settled edit thread identity not established"))
	}
	seen := make(map[string]bool, len(turns))
	for _, turn := range turns {
		raw, err := json.Marshal(turn)
		var decoded editTurn
		if observer.budget(len(raw)) != nil || err != nil || json.Unmarshal(raw, &decoded) != nil || seen[decoded.ID] {
			return observer.reject(failedObservation("invalid_settled_edit_thread", "live: settled edit thread identity not established"))
		}
		seen[decoded.ID] = true
		var ledger *sdkEditLedger
		for _, candidate := range observer.ledgers {
			if candidate.reconciled && decoded.ID == candidate.probe.turnID {
				ledger = candidate
				break
			}
		}
		if ledger == nil {
			return observer.reject(failedObservation("invalid_settled_edit_thread", "live: settled edit thread identity not established"))
		}
		if err := observer.accept(ledger, sdkEditObservation{ThreadID: threadID, Turn: decoded, method: "turn/completed"}); err != nil {
			return err
		}
	}
	return observer.cause
}

func (ledger *sdkEditLedger) proof() error {
	observation := &ledger.observation
	if !ledger.reconciled || !observation.completed {
		return failedObservation("edit_terminal_missing", "live: edit terminal evidence missing")
	}
	if ledger.probe.fixture.decline && ledger.evidence.Approvals == 0 {
		return failedObservation("edit_decline_missing", "live: no matching file-change approval was declined")
	}
	reply, err := finalProductReply(productTurn{Items: observation.replies})
	if err != nil {
		return failedObservation("edit_reply_missing", "live: edit final reply unavailable")
	}
	observation.replyBytes = len(reply)
	return nil
}

// Validate causal completion separately from the integration's file-byte oracle.
func (observer *sdkEditObserver) requireTurnProof() error {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	if observer.cause != nil {
		return observer.cause
	}
	if len(observer.ledgers) == 0 {
		return observer.reject(failedObservation("edit_terminal_missing", "live: edit terminal evidence missing"))
	}
	for _, ledger := range observer.ledgers {
		if err := ledger.proof(); err != nil {
			return observer.reject(err)
		}
	}
	return nil
}

func (observer *sdkEditObserver) snapshot() (StructuredEditEvidence, error) {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	var evidence StructuredEditEvidence
	if len(observer.ledgers) != 0 {
		first := observer.ledgers[0]
		for index, ledger := range observer.ledgers {
			reply, err := finalProductReply(productTurn{Items: ledger.observation.replies})
			observer.diagnostics.Record("oracle_reply", map[string]any{"scenario": "edit", "turn": index + 1, "text": reply, "available": err == nil})
		}
		evidence = observer.ledgers[len(observer.ledgers)-1].evidence
		evidence.Turns = len(observer.ledgers)
		evidence.FirstCompleted, evidence.FirstReplyBytes = first.evidence.FirstCompleted, first.observation.replyBytes
		evidence.ReplyBytes = observer.ledgers[len(observer.ledgers)-1].observation.replyBytes
		evidence.Approvals = first.evidence.Approvals
		evidence.FileChanges = 0
		for _, ledger := range observer.ledgers {
			evidence.FileChanges += ledger.evidence.FileChanges
		}

	}
	return evidence, observer.cause
}

func (observer *sdkEditObserver) noteCallback(ctx context.Context) {
	if observer.clientDone == nil && ctx != nil {
		observer.clientDone = ctx.Done()
	}
}

func (observer *sdkEditObserver) failure(cause error) error {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	if observer.cause != nil {
		return observer.cause
	}
	return sdkFailure(cause)
}
