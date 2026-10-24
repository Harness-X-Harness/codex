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

func editSDKRaw(t *testing.T, observer *sdkEditObserver, turn string, item editItem) error {
	t.Helper()
	raw, err := json.Marshal(map[string]any{"method": codexsdk.RawResponseItemCompletedExtension, "params": map[string]any{"threadId": "PRIVATE_CANARY_thread", "turnId": turn, "item": item}})
	var envelope protocolv2.JSONValue
	if err != nil || json.Unmarshal(raw, &envelope) != nil {
		t.Fatal("raw extension fixture could not be encoded")
	}
	return observer.notification(context.Background(), codexsdk.RawNotification{Method: codexsdk.RawResponseItemCompletedExtension, Envelope: envelope})
}

func editSDKCall(t *testing.T, observer *sdkEditObserver) editItem {
	t.Helper()
	args, err := json.Marshal(map[string]any{"file_path": observer.fixture.path, "old_string": editOld, "new_string": editNew, "replace_all": observer.fixture.replaceAll})
	if err != nil {
		t.Fatal("edit argument fixture could not be encoded")
	}
	return editItem{Type: "function_call", CallID: "PRIVATE_CANARY_call", Name: "structured_edit", Arguments: string(args)}
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
	output := editItem{Type: "function_call_output", CallID: "PRIVATE_CANARY_call", Output: json.RawMessage(`"PRIVATE_CANARY_result"`)}
	// Output-before-call is harmless: eventual pairing, not callback order,
	// establishes tool-result authority.
	if editSDKRaw(t, observer, "first", output) != nil || editSDKRaw(t, observer, "first", editSDKCall(t, observer)) != nil ||
		editSDKTerminal(observer, editSDKTurn("first", observer.fixture.decline)) != nil || observer.reconcile(protocolv2.Turn{ID: "first", Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}}) != nil ||
		observer.validateSettled("PRIVATE_CANARY_thread", []protocolv2.Turn{editSDKTurn("first", observer.fixture.decline)}) != nil || observer.requireTurnProof() != nil {
		t.Fatal("authorized first-turn causal story failed")
	}
}

func TestSDKEditObserverPositiveStory(t *testing.T) {
	for name, fixture := range map[string]editFixture{"continuation": {continuation: true}, "pinned": {}, "replace_all": {replaceAll: true}} {
		t.Run(name, func(t *testing.T) {
			observer := editSDKObserver(t, fixture)
			editSDKStory(t, observer)
			want := StructuredEditEvidence{Evidence: Evidence{Turns: 1, Stage: "edit_turn_completed", FirstCompleted: true, FirstReplyBytes: 14, ReplyBytes: 14}, TurnCompleted: true, Calls: 1, PairedOutputs: 1, FileChanges: 1}
			if fixture.continuation {
				if observer.beginTurn() != nil || editSDKTerminal(observer, editSDKTurn("second", false)) != nil {
					t.Fatal("valid continuation not admitted")
				}
				early, _ := observer.snapshot()
				if early.TurnCompleted || !early.FirstCompleted {
					t.Fatal("early continuation credited completion before reconciliation")
				}
				if observer.reconcile(protocolv2.Turn{ID: "second", Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}}) != nil || observer.validateSettled("PRIVATE_CANARY_thread", []protocolv2.Turn{editSDKTurn("first", false), editSDKTurn("second", false)}) != nil || observer.requireTurnProof() != nil {
					t.Fatal("same-thread continuation lost first-turn evidence")
				}
				want.Turns, want.Stage = 2, "edit_continuation_completed"
			}
			got, err := observer.snapshot()
			if err != nil || !reflect.DeepEqual(got, want) {
				t.Fatalf("causal evidence mismatch: got %+v; want %+v", got, want)
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
			if err != nil || editSDKRaw(t, observer, "first", editSDKCall(t, observer)) != nil || editSDKRaw(t, observer, "first", editItem{Type: "function_call_output", CallID: "PRIVATE_CANARY_call", Output: json.RawMessage(`"declined"`)}) != nil || observer.requireTurnProof() != nil {
				t.Fatal("reconciled decline lost exact call/change/approval correlation")
			}
			if got, err := observer.snapshot(); err != nil || got.Approvals != 1 {
				t.Fatal("actual matching decline was not credited once")
			}
		})
	}
}

func TestSDKEditObserverDuplicateAndConflict(t *testing.T) {
	for _, kind := range []string{"same_call", "changed_args", "second_call", "same_output", "changed_output", "second_output", "same_item", "same_item_different_diff", "changed_status", "same_terminal", "failed_terminal"} {
		t.Run(kind, func(t *testing.T) {
			observer := editSDKObserver(t, editFixture{})
			editSDKStory(t, observer)
			var err error
			switch {
			case strings.Contains(kind, "call") || kind == "changed_args":
				item := editSDKCall(t, observer)
				if kind == "changed_args" {
					item.Arguments = strings.ReplaceAll(item.Arguments, editOld, "PRIVATE_CANARY")
				}
				if kind == "second_call" {
					item.CallID = "different"
				}
				err = editSDKRaw(t, observer, "first", item)
			case strings.Contains(kind, "output"):
				item := editItem{Type: "function_call_output", CallID: "PRIVATE_CANARY_call", Output: json.RawMessage(`"PRIVATE_CANARY_result"`)}
				if kind == "changed_output" {
					item.Output = json.RawMessage(`"different"`)
				}
				if kind == "second_output" {
					item.CallID = "different"
				}
				err = editSDKRaw(t, observer, "first", item)
			default:
				turn := editSDKTurn("first", false)
				if kind == "failed_terminal" {
					turn.Status = protocolv2.TurnStatusFailed
				}
				if kind == "changed_status" {
					turn = editSDKTurn("first", true)
				}
				if kind == "same_item_different_diff" {
					change, _ := turn.Items[1].AsFileChange()
					change.Changes[0].Diff = "another private rendering"
					turn.Items[1] = protocolv2.NewThreadItemFileChange(change)
				}
				if strings.Contains(kind, "item") || kind == "changed_status" {
					err = observer.typedNotification(context.Background(), protocolv2.NewServerNotificationItemCompleted(protocolv2.ServerNotificationItemCompleted{Params: protocolv2.ItemCompletedNotification{ThreadID: "PRIVATE_CANARY_thread", TurnID: "first", Item: turn.Items[1], CompletedAtMS: 1}}))
				} else {
					err = editSDKTerminal(observer, turn)
				}
			}
			if strings.HasPrefix(kind, "same_") {
				if err != nil || observer.requireTurnProof() != nil {
					t.Fatal("identical duplicate invalidated causal story")
				}
			} else if err == nil || observer.requireTurnProof() == nil {
				t.Fatal("material conflicting evidence accepted")
			}
		})
	}
}

func TestSDKEditObserverRetainedFirstTurn(t *testing.T) {
	for _, source := range []string{"raw_call", "raw_output", "terminal", "settled"} {
		t.Run(source, func(t *testing.T) {
			observer := editSDKObserver(t, editFixture{continuation: true})
			editSDKStory(t, observer)
			if observer.beginTurn() != nil || observer.reconcile(protocolv2.Turn{ID: "second", Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}}) != nil {
				t.Fatal("continuation admission failed")
			}
			bad := editSDKTurn("first", false)
			bad.Status = protocolv2.TurnStatusFailed
			var err error
			switch source {
			case "raw_call":
				item := editSDKCall(t, observer)
				item.CallID = "late_other"
				err = editSDKRaw(t, observer, "first", item)
			case "raw_output":
				err = editSDKRaw(t, observer, "first", editItem{Type: "function_call_output", CallID: "PRIVATE_CANARY_call", Output: json.RawMessage(`"late different"`)})
			case "terminal":
				err = editSDKTerminal(observer, bad)
			case "settled":
				err = observer.validateSettled("PRIVATE_CANARY_thread", []protocolv2.Turn{bad, editSDKTurn("second", false)})
			}
			_ = editSDKTerminal(observer, editSDKTurn("second", false))
			got, sticky := observer.snapshot()
			if err == nil || sticky == nil || !got.FirstCompleted || got.Completed || observer.requireTurnProof() == nil {
				t.Fatal("second turn erased material first-turn contradiction")
			}
		})
	}
}

func TestSDKEditObserverBudgetsAndSafeEvidence(t *testing.T) {
	for _, control := range []string{"count", "bytes", "arguments", "output", "early_arguments", "early_output", "inline_items", "settled_items"} {
		t.Run(control, func(t *testing.T) {
			observer := editSDKObserver(t, editFixture{})
			var err error
			switch control {
			case "count", "bytes":
				item := editItem{Type: "reasoning", Text: "PRIVATE_CANARY"}
				if control == "bytes" {
					item.Text = strings.Repeat("PRIVATE_CANARY", 1<<16)
				}
				for index := 0; index <= sdkEditObservationLimit && err == nil; index++ {
					err = editSDKRaw(t, observer, "first", item)
				}
			case "arguments", "output", "early_arguments", "early_output":
				item := editSDKCall(t, observer)
				if strings.HasSuffix(control, "arguments") {
					item.Arguments = strings.Repeat("PRIVATE_CANARY", 4096)
				} else {
					item = editItem{Type: "function_call_output", CallID: "PRIVATE_CANARY_call", Output: json.RawMessage(fmt.Sprintf("%q", strings.Repeat("PRIVATE_CANARY", 64<<10)))}
				}
				if !strings.HasPrefix(control, "early_") {
					_ = observer.reconcile(protocolv2.Turn{ID: "first", Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}})
				}
				err = editSDKRaw(t, observer, "first", item)
			case "inline_items", "settled_items":
				turn := editSDKTurn("first", false)
				turn.Items = make([]protocolv2.ThreadItem, sdkEditObservationLimit)
				for index := range turn.Items {
					turn.Items[index] = editSDKTurn("second", false).Items[0]
				}
				if control == "inline_items" {
					err = observer.reconcile(turn)
				} else {
					_ = observer.reconcile(protocolv2.Turn{ID: "first", Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}})
					err = observer.validateSettled("PRIVATE_CANARY_thread", []protocolv2.Turn{turn})
				}
			}
			got, sticky := observer.snapshot()
			failure := observer.failure(fmt.Errorf("PRIVATE_CANARY SDK error"))
			safe, marshalErr := json.Marshal([]any{got, DescribeFailure(failure), failure.Error()})
			if err == nil || sticky == nil || got.Completed || marshalErr != nil || strings.Contains(string(safe), "PRIVATE_CANARY") {
				t.Fatal("bounded failure lost sticky cause or exported private content")
			}
		})
	}
}

func TestSDKEditObserverIdentityAndContinuation(t *testing.T) {
	for _, control := range []string{"wrong_thread", "wrong_turn", "second_edit", "reused_turn"} {
		t.Run(control, func(t *testing.T) {
			observer := editSDKObserver(t, editFixture{continuation: true})
			editSDKStory(t, observer)
			if observer.beginTurn() != nil {
				t.Fatal("continuation admission failed")
			}
			turn := "second"
			if control == "reused_turn" {
				turn = "first"
			}
			err := observer.reconcile(protocolv2.Turn{ID: turn, Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}})
			if control != "reused_turn" {
				if err != nil {
					t.Fatal("distinct continuation identity rejected")
				}
				switch control {
				case "wrong_thread":
					err = observer.typedNotification(context.Background(), protocolv2.NewServerNotificationTurnCompleted(protocolv2.ServerNotificationTurnCompleted{Params: protocolv2.TurnCompletedNotification{ThreadID: "unrelated", Turn: editSDKTurn("second", false)}}))
				case "wrong_turn":
					err = editSDKTerminal(observer, editSDKTurn("unrelated", false))
				case "second_edit":
					err = editSDKRaw(t, observer, "second", editSDKCall(t, observer))
				}
			}
			if err == nil || observer.requireTurnProof() == nil {
				t.Fatal("unattributed or unauthorized continuation accepted")
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

func TestSDKEditObserverIncompleteProof(t *testing.T) {
	for _, missing := range []string{"call", "output", "change", "reply", "terminal", "approval"} {
		t.Run(missing, func(t *testing.T) {
			observer := editSDKObserver(t, editFixture{decline: missing == "approval"})
			if missing == "approval" {
				if _, err := editSDKApproval(observer, "unrelated_turn", "PRIVATE_CANARY_call", "PRIVATE_CANARY_thread"); err != nil {
					t.Fatal("unrelated approval was not safely denied")
				}
			}
			if observer.reconcile(protocolv2.Turn{ID: "first", Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}}) != nil {
				t.Fatal("turn admission failed")
			}
			if missing != "call" {
				_ = editSDKRaw(t, observer, "first", editSDKCall(t, observer))
			}
			if missing != "output" {
				_ = editSDKRaw(t, observer, "first", editItem{Type: "function_call_output", CallID: "PRIVATE_CANARY_call", Output: json.RawMessage(`"result"`)})
			}
			turn := editSDKTurn("first", missing == "approval")
			if missing == "change" {
				turn.Items = turn.Items[:1]
			}
			if missing == "reply" {
				turn.Items = turn.Items[1:]
			}
			if missing != "terminal" {
				_ = editSDKTerminal(observer, turn)
			}
			if observer.requireTurnProof() == nil {
				t.Fatal("missing causal business evidence proved completion")
			}
		})
	}
}

func TestSDKEditObserverSemanticReplay(t *testing.T) {
	for _, kind := range []string{"arguments", "output_form", "output_optional_name", "file_path", "irrelevant_call_fields"} {
		t.Run(kind, func(t *testing.T) {
			observer := editSDKObserver(t, editFixture{})
			editSDKStory(t, observer)
			var err error
			switch kind {
			case "arguments", "irrelevant_call_fields":
				item := editSDKCall(t, observer)
				if kind == "arguments" {
					item.Arguments = fmt.Sprintf(` { "new_string":%q, "file_path":"./structured_edit_fixture.txt", "old_string":%q } `, editNew, editOld)
				} else {
					item.ID, item.Status = "transport-item-id", "completed"
				}
				err = editSDKRaw(t, observer, "first", item)
			case "output_form", "output_optional_name":
				item := editItem{Type: "function_call_output", CallID: "PRIVATE_CANARY_call", Output: json.RawMessage(`[{"type":"input_text","text":"PRIVATE_CANARY_result"}]`)}
				if kind == "output_optional_name" {
					item.Name = "structured_edit"
				}
				err = editSDKRaw(t, observer, "first", item)
			case "file_path":
				turn := editSDKTurn("first", false)
				change, _ := turn.Items[1].AsFileChange()
				change.Changes[0].Path = "./" + editFile
				turn.Items[1] = protocolv2.NewThreadItemFileChange(change)
				err = editSDKTerminal(observer, turn)
			}
			if err != nil || observer.requireTurnProof() != nil {
				t.Fatal("equivalent operation facts rejected")
			}
		})
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

func TestSDKEditObserverSemanticReplayRejectsInvalidAuthority(t *testing.T) {
	for _, kind := range []string{"output_array_of_strings", "output_name", "output_namespace", "call_namespace", "call_name", "file_other_root"} {
		t.Run(kind, func(t *testing.T) {
			observer := editSDKObserver(t, editFixture{})
			editSDKStory(t, observer)
			var err error
			switch kind {
			case "file_other_root":
				turn := editSDKTurn("first", false)
				change, _ := turn.Items[1].AsFileChange()
				change.Changes[0].Path = filepath.Join(string(filepath.Separator), "OTHER", editFile)
				turn.Items[1] = protocolv2.NewThreadItemFileChange(change)
				err = editSDKTerminal(observer, turn)
			case "call_namespace", "call_name":
				item := editSDKCall(t, observer)
				if kind == "call_name" {
					item.Name = "local__structured_edit__deadbeefcafe"
				} else {
					namespace := "other"
					item.Namespace = &namespace
				}
				err = editSDKRaw(t, observer, "first", item)
			default:
				item := editItem{Type: "function_call_output", CallID: "PRIVATE_CANARY_call", Output: json.RawMessage(`"PRIVATE_CANARY_result"`)}
				switch kind {
				case "output_array_of_strings":
					item.Output = json.RawMessage(`["PRIVATE_CANARY_result"]`)
				case "output_name":
					item.Name = "other"
				case "output_namespace":
					namespace := "other"
					item.Namespace = &namespace
				}
				err = editSDKRaw(t, observer, "first", item)
			}
			if err == nil || observer.requireTurnProof() == nil {
				t.Fatal("semantic normalization erased invalid authority or target")
			}
		})
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
