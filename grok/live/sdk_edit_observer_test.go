package live

import (
	"context"
	"encoding/json"
	"fmt"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func editSDKObserver(t *testing.T, fixture editFixture) *sdkEditObserver {
	t.Helper()
	fixture.path = filepath.Join(string(filepath.Separator), "PRIVATE_CANARY", editFile)
	observer := newSDKEditObserver(&fixture)
	if observer.bindThread("PRIVATE_CANARY_thread") != nil || observer.beginTurn() != nil {
		t.Fatal("fixture observer did not admit first turn")
	}
	return observer
}

func editSDKTurn(turn string, declined bool) protocolv2.Turn {
	status := protocolv2.PatchApplyStatusCompleted
	if declined {
		status = protocolv2.PatchApplyStatusDeclined
	}
	items := []protocolv2.ThreadItem{protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "PRIVATE_CANARY_reply", Text: "PRIVATE_CANARY", Phase: protocolv2.Value(protocolv2.MessagePhaseFinalAnswer)})}
	if turn == "first" {
		items = append(items, protocolv2.NewThreadItemFileChange(protocolv2.ThreadItemFileChange{ID: "PRIVATE_CANARY_call", Status: status, Changes: []protocolv2.FileUpdateChange{
			{Path: filepath.Join(string(filepath.Separator), "PRIVATE_CANARY", editFile), Diff: "PRIVATE_CANARY_diff", Kind: protocolv2.NewPatchChangeKindUpdate(protocolv2.PatchChangeKindUpdate{})},
		}}))
	}
	return protocolv2.Turn{ID: turn, Status: protocolv2.TurnStatusCompleted, Items: items}
}

func editSDKTerminal(observer *sdkEditObserver, turn protocolv2.Turn) error {
	return observer.typedNotification(context.Background(), protocolv2.NewServerNotificationTurnCompleted(protocolv2.ServerNotificationTurnCompleted{
		Params: protocolv2.TurnCompletedNotification{ThreadID: "PRIVATE_CANARY_thread", Turn: turn},
	}))
}

func editSDKApproval(observer *sdkEditObserver, turn, item, thread string) (codexsdk.ServerRequestResponse, error) {
	return observer.request(context.Background(), protocolv2.NewServerRequestItemFileChangeRequestApproval(protocolv2.ServerRequestItemFileChangeRequestApproval{
		ID: protocolv2.NewRequestIdInt64(1), Params: protocolv2.FileChangeRequestApprovalParams{ThreadID: thread, TurnID: turn, ItemID: item, StartedAtMS: 1},
	}))
}

func editSDKStory(t *testing.T, observer *sdkEditObserver) {
	t.Helper()
	if editSDKTerminal(observer, editSDKTurn("first", observer.fixture.decline)) != nil ||
		observer.reconcile(protocolv2.Turn{ID: "first", Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}}) != nil ||
		observer.validateSettled("PRIVATE_CANARY_thread", []protocolv2.Turn{editSDKTurn("first", observer.fixture.decline)}) != nil || observer.requireTurnProof() != nil {
		t.Fatalf("completed public turn lost its proof: %v", observer.cause)
	}
}

func TestSDKEditObserverPositiveStory(t *testing.T) {
	for name, fixture := range map[string]editFixture{"continuation": {continuation: true}, "pinned": {}, "replace_all": {replaceAll: true}} {
		t.Run(name, func(t *testing.T) {
			observer := editSDKObserver(t, fixture)
			editSDKStory(t, observer)
			if fixture.continuation {
				if observer.beginTurn() != nil || observer.reconcile(editSDKTurn("second", false)) != nil || observer.requireTurnProof() != nil {
					t.Fatal("same-thread continuation failed")
				}
			}
			got, err := observer.snapshot()
			if err != nil || !got.FirstCompleted || !got.TurnCompleted || got.FileChanges != 1 || got.ReplyBytes == 0 || got.Completed {
				t.Fatalf("incorrect partial evidence: %+v %v", got, err)
			}
		})
	}
}

func TestSDKEditObserverAllowsEquivalentExecution(t *testing.T) {
	observer := editSDKObserver(t, editFixture{})
	turn := editSDKTurn("first", false)
	// Distinct completed changes and an auxiliary command are not a failure.
	change, _ := turn.Items[1].AsFileChange()
	change.ID = "second-edit"
	turn.Items = append(turn.Items, protocolv2.NewThreadItemFileChange(change),
		protocolv2.NewThreadItemCommandExecution(protocolv2.ThreadItemCommandExecution{ID: "inspect", Command: "inspect fixture", CommandActions: []protocolv2.CommandAction{}, CWD: observer.fixture.path, Status: protocolv2.CommandExecutionStatusCompleted}))
	if observer.reconcile(turn) != nil || observer.requireTurnProof() != nil {
		t.Fatal("equivalent execution path was rejected")
	}
	// This is only terminal proof; sdkStructuredEdit must still read exact bytes.
	got, _ := observer.snapshot()
	if got.FileChanges != 2 || got.Completed || got.BytesMatch {
		t.Fatal("intermediate activity manufactured filesystem proof")
	}
}

func TestSDKEditObserverDeclineBoundary(t *testing.T) {
	for _, mode := range []string{"no_change_item", "multiple_declines", "completed_write", "missing_approval", "unrelated_approval"} {
		t.Run(mode, func(t *testing.T) {
			observer := editSDKObserver(t, editFixture{decline: true})
			if mode != "missing_approval" {
				turnID := "first"
				if mode == "unrelated_approval" {
					turnID = "other"
				}
				if _, err := editSDKApproval(observer, turnID, "attempt-a", "PRIVATE_CANARY_thread"); err != nil {
					t.Fatal(err)
				}
				if mode == "multiple_declines" {
					if _, err := editSDKApproval(observer, turnID, "attempt-b", "PRIVATE_CANARY_thread"); err != nil {
						t.Fatal(err)
					}
				}
			}
			turn := editSDKTurn("first", mode != "completed_write")
			if mode == "no_change_item" {
				turn.Items = turn.Items[:1]
			}
			err := observer.reconcile(turn)
			if err == nil {
				err = observer.requireTurnProof()
			}
			wantFailure := mode == "completed_write" || mode == "missing_approval" || mode == "unrelated_approval"
			if (err != nil) != wantFailure {
				t.Fatalf("incorrect denial proof: %v", err)
			}
		})
	}
}

func TestSDKEditObserverIncompleteProof(t *testing.T) {
	for _, missing := range []string{"reply", "terminal"} {
		t.Run(missing, func(t *testing.T) {
			observer := editSDKObserver(t, editFixture{})
			if observer.reconcile(protocolv2.Turn{ID: "first", Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}}) != nil {
				t.Fatal("start failed")
			}
			turn := editSDKTurn("first", false)
			if missing == "reply" {
				turn.Items = turn.Items[1:]
				_ = editSDKTerminal(observer, turn)
			}
			if observer.requireTurnProof() == nil {
				t.Fatal("missing terminal or final answer passed")
			}
		})
	}
}

func TestSDKEditObserverIdentityAndContinuation(t *testing.T) {
	for _, mode := range []string{"wrong_thread", "wrong_turn", "reused_turn", "late_failed_terminal"} {
		t.Run(mode, func(t *testing.T) {
			observer := editSDKObserver(t, editFixture{continuation: true})
			editSDKStory(t, observer)
			if observer.beginTurn() != nil {
				t.Fatal("continuation failed")
			}
			id := "second"
			if mode == "reused_turn" {
				id = "first"
			}
			err := observer.reconcile(protocolv2.Turn{ID: id, Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}})
			if err == nil {
				switch mode {
				case "wrong_thread":
					err = observer.typedNotification(context.Background(), protocolv2.NewServerNotificationTurnCompleted(protocolv2.ServerNotificationTurnCompleted{Params: protocolv2.TurnCompletedNotification{ThreadID: "other", Turn: editSDKTurn("second", false)}}))
				case "wrong_turn":
					err = editSDKTerminal(observer, editSDKTurn("other", false))
				case "late_failed_terminal":
					turn := editSDKTurn("first", false)
					turn.Status = protocolv2.TurnStatusFailed
					err = editSDKTerminal(observer, turn)
				}
			}
			if err == nil || observer.requireTurnProof() == nil {
				t.Fatal("unrelated or failed turn passed")
			}
		})
	}
}

func TestSDKEditObserverBudgetsAndSafeEvidence(t *testing.T) {
	for _, mode := range []string{"count", "bytes"} {
		t.Run(mode, func(t *testing.T) {
			observer := editSDKObserver(t, editFixture{})
			var err error
			size := 1
			if mode == "bytes" {
				size = sdkEditByteLimit + 1
			}
			for i := 0; i <= sdkEditObservationLimit && err == nil; i++ {
				err = observer.budget(size)
			}
			got, sticky := observer.snapshot()
			failure := observer.failure(fmt.Errorf("PRIVATE_CANARY"))
			safe, _ := json.Marshal([]any{got, DescribeFailure(failure), failure.Error()})
			if err == nil || sticky == nil || got.Completed || strings.Contains(string(safe), "PRIVATE_CANARY") || DescribeFailure(err).Code != "edit_evidence_budget_exceeded" {
				t.Fatal("retained-state budget or privacy boundary failed")
			}
		})
	}
}

func TestSDKEditObserverDuplicateRepresentations(t *testing.T) {
	for _, mode := range []string{"early_items", "inline_items", "settled_items", "irrelevant_items"} {
		t.Run(mode, func(t *testing.T) {
			observer := editSDKObserver(t, editFixture{})
			turn := editSDKTurn("first", false)
			for i := 0; i <= sdkEditObservationLimit; i++ {
				item := turn.Items[0]
				if mode == "irrelevant_items" {
					item = protocolv2.NewThreadItemCommandExecution(protocolv2.ThreadItemCommandExecution{ID: fmt.Sprintf("tool-%d", i), Command: "inspect", CWD: observer.fixture.path, CommandActions: []protocolv2.CommandAction{}, Status: protocolv2.CommandExecutionStatusCompleted})
				}
				turn.Items = append(turn.Items, item)
			}
			var err error
			switch mode {
			case "early_items":
				err = editSDKTerminal(observer, turn)
				if err == nil {
					err = observer.reconcile(protocolv2.Turn{ID: "first", Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}})
				}
			case "settled_items":
				err = observer.reconcile(protocolv2.Turn{ID: "first", Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}})
				if err == nil {
					err = observer.validateSettled("PRIVATE_CANARY_thread", []protocolv2.Turn{turn})
				}
			default:
				err = observer.reconcile(turn)
			}
			if err != nil || observer.requireTurnProof() != nil || observer.count != 2 {
				t.Fatalf("duplicate or irrelevant representations consumed retained-state budget: count=%d err=%v", observer.count, err)
			}
			if err := observer.validateSettled("PRIVATE_CANARY_thread", []protocolv2.Turn{turn}); err != nil || observer.count != 2 {
				t.Fatal("settled replay charged a second copy")
			}
		})
	}
}

func TestSDKEditObserverSelectedReplyAccounting(t *testing.T) {
	for _, mode := range []string{"large_discarded_legacy", "large_commentary", "empty_final_keeps_legacy", "released_legacy_conflict"} {
		t.Run(mode, func(t *testing.T) {
			observer := editSDKObserver(t, editFixture{})
			turn := editSDKTurn("first", false)
			final := turn.Items[0]
			legacy := invoiceMessage("legacy", "", strings.Repeat("x", sdkEditByteLimit))
			switch mode {
			case "large_discarded_legacy":
				turn.Items = []protocolv2.ThreadItem{legacy, final}
			case "large_commentary":
				turn.Items = []protocolv2.ThreadItem{invoiceMessage("commentary", "commentary", strings.Repeat("x", sdkEditByteLimit)), final}
			case "empty_final_keeps_legacy":
				turn.Items = []protocolv2.ThreadItem{invoiceMessage("legacy", "", "answer"), invoiceMessage("empty", "final_answer", " \n ")}
			case "released_legacy_conflict":
				legacy = invoiceMessage("legacy", "", strings.Repeat("x", sdkEditByteLimit/2))
				if observer.reconcile(protocolv2.Turn{ID: "first", Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}}) != nil {
					t.Fatal("start failed")
				}
				if editSDKTerminal(observer, protocolv2.Turn{ID: "first", Status: protocolv2.TurnStatusCompleted, Items: []protocolv2.ThreadItem{legacy}}) != nil || editSDKTerminal(observer, turn) != nil {
					t.Fatal("final selection failed")
				}
				if observer.bytes >= sdkEditByteLimit/2 {
					t.Fatal("superseded legacy text retained its byte charge")
				}
				if editSDKTerminal(observer, protocolv2.Turn{ID: "first", Status: protocolv2.TurnStatusCompleted, Items: []protocolv2.ThreadItem{invoiceMessage("legacy", "", "changed")}}) == nil || observer.requireTurnProof() == nil {
					t.Fatal("discarding legacy text also discarded its conflict witness")
				}
				return
			}
			if err := observer.reconcile(turn); err != nil || observer.requireTurnProof() != nil || observer.bytes > 2048 {
				t.Fatalf("unselected text consumed retained answer budget: bytes=%d err=%v", observer.bytes, err)
			}
		})
	}
}

func TestSDKEditObserverEarlyDeclineReconciliation(t *testing.T) {
	for _, scenario := range []string{"first", "wrong_first", "wrong", ""} {
		t.Run("actual_"+scenario, func(t *testing.T) {
			actual := scenario
			if scenario == "wrong_first" {
				actual = "first"
			}
			observer := editSDKObserver(t, editFixture{decline: true})
			want := codexsdk.FileChangeApprovalResponse(protocolv2.FileChangeRequestApprovalResponse{Decision: protocolv2.FileChangeApprovalDecisionDecline})
			requests := [][2]string{{"unrelated", "first"}, {"PRIVATE_CANARY_thread", "first"}, {"PRIVATE_CANARY_thread", "first"}, {"PRIVATE_CANARY_thread", "unrelated_turn"}}
			if scenario == "wrong_first" {
				requests[0], requests[3] = requests[3], requests[0]
			}
			for _, request := range requests {
				response, err := editSDKApproval(observer, request[1], "PRIVATE_CANARY_call", request[0])
				if err != nil || !reflect.DeepEqual(response, want) {
					t.Fatal("file approval did not return typed safe decline")
				}
			}
			if editSDKTerminal(observer, editSDKTurn("first", true)) != nil {
				t.Fatal("valid early terminal rejected")
			}
			before, _ := observer.snapshot()
			if before.FirstCompleted || before.TurnCompleted || before.Approvals != 0 {
				t.Fatal("duplicate decline counted twice or credited premature completion")
			}
			err := observer.reconcile(protocolv2.Turn{ID: actual, Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}})
			if actual != "first" {
				code := "edit_turn_identity_missing"
				if actual != "" {
					code = "unrelated_edit_turn"
				}
				if DescribeFailure(err).Code != code {
					t.Fatal("unreconciled approval received actual turn authority")
				}
				return
			}
			if err != nil || observer.requireTurnProof() != nil {
				t.Fatal("reconciled decline lost matching-turn approval evidence")
			}
			if got, err := observer.snapshot(); err != nil || got.Approvals != 1 {
				t.Fatal("actual matching decline was not credited once")
			}
		})
	}
}

func TestSDKEditObserverConcurrentSnapshots(t *testing.T) {
	observer := editSDKObserver(t, editFixture{continuation: true})
	stop, joined := make(chan struct{}), make(chan struct{})
	go func() {
		defer close(joined)
		for {
			select {
			case <-stop:
				return
			default:
				_, _ = observer.snapshot()
			}
		}
	}()
	defer func() { close(stop); <-joined }()
	editSDKStory(t, observer)
	if observer.beginTurn() != nil || observer.reconcile(editSDKTurn("second", false)) != nil || observer.requireTurnProof() != nil {
		t.Fatal("concurrent snapshot lost causal turn proof")
	}
	got, err := observer.snapshot()
	if err != nil || !got.FirstCompleted || !got.TurnCompleted || got.Completed {
		t.Fatal("concurrent callbacks exported inconsistent completion facts")
	}
}

func TestSDKEditObserverReplySelection(t *testing.T) {
	observer := editSDKObserver(t, editFixture{})
	editSDKStory(t, observer)
	turn := editSDKTurn("first", false)
	turn.Items = append(turn.Items,
		protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "commentary", Text: "not the answer", Phase: protocolv2.Value(protocolv2.MessagePhaseCommentary)}),
		protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "legacy", Text: "legacy fallback"}),
		protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "second-final", Text: "Additional confirmation.", Phase: protocolv2.Value(protocolv2.MessagePhaseFinalAnswer)}))
	if editSDKTerminal(observer, turn) != nil || observer.requireTurnProof() != nil {
		t.Fatal("final answer erased by commentary or legacy text")
	}
	got, err := observer.snapshot()
	if err != nil || got.ReplyBytes != len("PRIVATE_CANARY\nAdditional confirmation.") {
		t.Fatal("reply not selected by shared final-answer semantics")
	}
}

func TestSDKEditObserverSettledTurnIdentity(t *testing.T) {
	for _, duplicate := range []bool{false, true} {
		observer := editSDKObserver(t, editFixture{continuation: true})
		editSDKStory(t, observer)
		if observer.beginTurn() != nil || observer.reconcile(protocolv2.Turn{ID: "second", Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}}) != nil || editSDKTerminal(observer, editSDKTurn("second", false)) != nil {
			t.Fatal("continuation failed")
		}
		turns := []protocolv2.Turn{editSDKTurn("second", false), editSDKTurn("first", false)}
		if duplicate {
			turns[1] = turns[0]
		}
		err := observer.validateSettled("PRIVATE_CANARY_thread", turns)
		if (err != nil) != duplicate {
			t.Fatal("settled proof used array position instead of unique submitted turn identities")
		}
	}
}

func TestSDKEditObserverEmptyFragmentsBeforeFinal(t *testing.T) {
	for _, phase := range []string{"", "final_answer"} {
		t.Run("phase_"+phase, func(t *testing.T) {
			observer := editSDKObserver(t, editFixture{})
			message := protocolv2.ThreadItemAgentMessage{ID: "empty-fragment", Text: " \n "}
			if phase != "" {
				message.Phase = protocolv2.Value(protocolv2.MessagePhaseFinalAnswer)
			}
			notification := protocolv2.NewServerNotificationItemCompleted(protocolv2.ServerNotificationItemCompleted{Params: protocolv2.ItemCompletedNotification{
				ThreadID: "PRIVATE_CANARY_thread", TurnID: "first", Item: protocolv2.NewThreadItemAgentMessage(message), CompletedAtMS: 1,
			}})
			if observer.typedNotification(context.Background(), notification) != nil {
				t.Fatal("empty fragment invalidated the turn before its final answer")
			}
			editSDKStory(t, observer)
			got, err := observer.snapshot()
			if err != nil || got.ReplyBytes != len("PRIVATE_CANARY") {
				t.Fatal("empty fragment erased a later complete answer")
			}
		})
	}
}

func TestSDKEditObserverConflictingCompletedReply(t *testing.T) {
	observer := editSDKObserver(t, editFixture{})
	editSDKStory(t, observer)
	turn := editSDKTurn("first", false)
	message, _ := turn.Items[0].AsAgentMessage()
	message.Text = "different completed answer"
	turn.Items[0] = protocolv2.NewThreadItemAgentMessage(message)
	if editSDKTerminal(observer, turn) == nil || observer.requireTurnProof() == nil {
		t.Fatal("same completed message identity accepted contradictory content")
	}
}

func TestSDKEditObserverUniqueEvidenceStillBounded(t *testing.T) {
	observer := editSDKObserver(t, editFixture{})
	if observer.reconcile(protocolv2.Turn{ID: "first", Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}}) != nil {
		t.Fatal("start failed")
	}
	var err error
	for i := 0; i <= sdkEditObservationLimit; i++ {
		item := invoiceMessage(fmt.Sprintf("commentary-%d", i), "commentary", "progress")
		err = observer.typedNotification(context.Background(), protocolv2.NewServerNotificationItemCompleted(protocolv2.ServerNotificationItemCompleted{Params: protocolv2.ItemCompletedNotification{ThreadID: "PRIVATE_CANARY_thread", TurnID: "first", Item: item, CompletedAtMS: 1}}))
		if i < sdkEditObservationLimit && err != nil {
			t.Fatal("bounded unique identities rejected early")
		}
	}
	if DescribeFailure(err).Code != "edit_evidence_budget_exceeded" || observer.requireTurnProof() == nil {
		t.Fatal("unbounded identity retention manufactured completed proof")
	}
}

func TestSDKEditObserverEarlyCandidatesStillBounded(t *testing.T) {
	observer := editSDKObserver(t, editFixture{})
	turn := editSDKTurn("first", false)
	turn.Items = []protocolv2.ThreadItem{invoiceMessage("large", "final_answer", strings.Repeat("x", sdkEditByteLimit))}
	if err := editSDKTerminal(observer, turn); DescribeFailure(err).Code != "edit_evidence_budget_exceeded" {
		t.Fatal("unbound retained payload lost its resource guard")
	}
}

func TestSDKEditObserverEarlyDuplicateDoesNotHideFailure(t *testing.T) {
	observer := editSDKObserver(t, editFixture{})
	turn := editSDKTurn("first", false)
	if editSDKTerminal(observer, turn) != nil || editSDKTerminal(observer, turn) != nil {
		t.Fatal("early duplicate terminal rejected")
	}
	turn.Status = protocolv2.TurnStatusFailed
	if editSDKTerminal(observer, turn) != nil {
		t.Fatal("unbound contradiction was not retained for identity reconciliation")
	}
	if err := observer.reconcile(protocolv2.Turn{ID: "first", Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}}); DescribeFailure(err).Code != "turn_unsuccessful" || observer.requireTurnProof() == nil {
		t.Fatal("early replay deduplication concealed a matching failed terminal")
	}
}
