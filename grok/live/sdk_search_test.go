package live

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"testing"
	"time"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func TestSearchRunTransportFailureLeavesOnlyMatchingTerminalsToObserver(t *testing.T) {
	failed := &codexsdk.TurnError{ThreadID: "thread", Turn: protocolv2.Turn{ID: "turn", Status: protocolv2.TurnStatusFailed}, Err: codexsdk.ErrTurnFailed}
	interrupted := &codexsdk.TurnError{ThreadID: "thread", Turn: protocolv2.Turn{ID: "turn", Status: protocolv2.TurnStatusInterrupted}, Err: codexsdk.ErrTurnInterrupted}
	wrongThread, wrongTurn, wrongStatus, wrongCause := *failed, *failed, *failed, *failed
	wrongThread.ThreadID, wrongTurn.Turn.ID = "other", "other"
	wrongStatus.Turn.Status = protocolv2.TurnStatusCompleted
	wrongCause.Err = errors.New("protocol failure")
	for _, tc := range []struct {
		name     string
		err      error
		observed bool
	}{
		{"failed", failed, true},
		{"interrupted", interrupted, true},
		{"other_thread", &wrongThread, false},
		{"other_turn", &wrongTurn, false},
		{"other_status", &wrongStatus, false},
		{"other_cause", &wrongCause, false},
		{"untyped", errors.New(failed.Error()), false},
		{"wrapped", fmt.Errorf("wrapped: %w", failed), false},
		{"joined", errors.Join(failed, errors.New("decode failure")), false},
		{"typed_nil", (*codexsdk.TurnError)(nil), false},
		{"success", nil, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			want := tc.err
			if tc.observed {
				want = nil
			}
			if got := searchRunTransportFailure(tc.err, "thread", "turn"); got != want {
				t.Fatalf("early failure = %v, want %v", got, want)
			}
		})
	}
}

func TestSearchRunTerminalClassificationSurvivesEitherSignalOrder(t *testing.T) {
	for _, order := range []string{"notification_first", "wait_first"} {
		t.Run(order, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			failures := make(chan error, 1)
			transport := &sdkProductObserver{ctx: ctx, changed: make(chan struct{}, 1), runFailures: failures}
			result := SearchEvidence{}
			probe := searchProbe{server: transport, threadID: "thread", evidence: &result.Evidence, result: &result}
			terminal := &codexsdk.TurnError{ThreadID: "thread", Turn: protocolv2.Turn{ID: "turn", Status: protocolv2.TurnStatusFailed}, Err: codexsdk.ErrTurnFailed}
			wait := func() {
				if err := searchRunTransportFailure(terminal, "thread", "turn"); err != nil {
					failures <- err
				}
			}
			notify := func() {
				params := json.RawMessage(`{"threadId":"thread","turn":{"id":"turn","status":"failed","items":[],"error":{"message":"PRIVATE_REJECTED","codexErrorInfo":"other"}}}`)
				var notification protocolv2.ServerNotification
				if err := json.Unmarshal(append(append([]byte(`{"method":"turn/completed","params":`), params...), '}'), &notification); err != nil {
					t.Fatal(err)
				}
				if err := transport.typedNotification(ctx, notification); err != nil {
					t.Fatal(err)
				}
			}
			if order == "notification_first" {
				notify()
				wait()
			} else {
				wait()
				notify()
			}
			if len(failures) != 0 {
				t.Fatal("matching terminal entered the competing transport failure channel")
			}
			err := probe.observeStarted(protocolv2.Turn{ID: "turn", Status: protocolv2.TurnStatusInProgress, Items: []protocolv2.ThreadItem{}})
			want := FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "other"}
			if DescribeFailure(err) != want || result.Completed || result.FirstCompleted || result.FollowupCompleted || result.CanonicalCalls != 0 || result.SettledTurns != 0 {
				t.Fatalf("terminal lost classification or gained credit: %+v, %v", result, err)
			}
		})
	}
}

func TestSearchPublicItemsAreBoundedAndDuplicatesDoNotAddCredit(t *testing.T) {
	result := SearchEvidence{}
	probe := searchProbe{turnID: "turn", result: &result, calls: map[string]map[string]bool{"turn": {}}}
	for i := 0; i < 64; i++ {
		var item protocolv2.ThreadItem
		raw := fmt.Sprintf(`{"type":"webSearch","id":"call-%d","query":"private query","action":{"type":"search"}}`, i)
		if err := json.Unmarshal([]byte(raw), &item); err != nil {
			t.Fatal(err)
		}
		if err := probe.publicSearch("turn", item); err != nil {
			t.Fatal(err)
		}
		if err := probe.publicSearch("turn", item); err != nil {
			t.Fatal(err)
		}
	}
	var extra protocolv2.ThreadItem
	if err := json.Unmarshal([]byte(`{"type":"webSearch","id":"overflow","query":"private query","action":{"type":"search"}}`), &extra); err != nil {
		t.Fatal(err)
	}
	if err := probe.publicSearch("turn", extra); err == nil {
		t.Fatal("unbounded public evidence")
	}
	if result.CanonicalCalls != 64 {
		t.Fatal("duplicates earned credit")
	}
	probe.settled = true
	if err := probe.publicSearch("turn", extra); err != nil || result.CanonicalCalls != 64 {
		t.Fatal("late observation changed settled result")
	}
}
