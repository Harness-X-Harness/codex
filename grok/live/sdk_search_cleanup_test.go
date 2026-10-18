package live

import (
	"encoding/json"
	"reflect"
	"testing"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func TestSearchCleanupRevalidatesObservedRawWithoutNewCredit(t *testing.T) {
	for _, tc := range []struct {
		name      string
		items     []string
		wantError bool
	}{
		{"observed_local", []string{`{"type":"custom_tool_call","id":"local","name":"apply_patch","input":"PRIVATE_INPUT"}`}, true},
		{"canonical_uncredited", []string{`{"type":"web_search_call","id":"call","status":"completed","action":{"type":"search","query":"PRIVATE_QUERY"}}`}, false},
		{"canonical_conflict", []string{`{"type":"web_search_call","id":"call","status":"completed","action":{"type":"search","query":"PRIVATE_QUERY"}}`, `{"type":"web_search_call","id":"call","status":"completed","action":{"type":"search","query":"PRIVATE_CHANGED"}}`}, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			evidence := Evidence{Stage: "search_turn_submitted"}
			result := SearchEvidence{Scenario: "web"}
			transport := &sdkProductObserver{}
			for _, item := range tc.items {
				params := []byte(`{"threadId":"thread","turnId":"turn","item":` + item + `}`)
				transport.pending = append(transport.pending, frame{Method: "rawResponseItem/completed", Params: params, size: len(params)})
			}
			probe := searchProbe{server: transport, threadID: "thread", turnID: "turn", scenario: "web", evidence: &evidence, result: &result, calls: map[string]map[string]string{"turn": {}}}
			err := probe.revalidate(nil)
			if (err != nil) != tc.wantError {
				t.Fatalf("validation=%v", err)
			}
			if result.CanonicalCalls != 0 || result.SettledTurns != 0 || evidence.Stage != "search_turn_submitted" || len(probe.calls["turn"]) != 0 {
				t.Fatal("cleanup created business credit")
			}
		})
	}
}

// Keep the observed queue empty so only the SDK-retained typed snapshot can
// detect contradictions. Cleanup validates that snapshot without granting credit.
func TestSearchCleanupRevalidatesRetainedTypedWithoutNewCredit(t *testing.T) {
	local := protocolv2.NewThreadItemCommandExecution(protocolv2.ThreadItemCommandExecution{
		ID: "local", Command: "PRIVATE_COMMAND", CWD: "/private", CommandActions: []protocolv2.CommandAction{}, Status: protocolv2.CommandExecutionStatusCompleted,
	})
	for _, identity := range []struct {
		name, threadID, turnID string
		correlated             bool
	}{
		{"current", "thread", "turn", true},
		{"previous", "thread", "previous", true},
		{"unrelated_thread", "other", "turn", false},
		{"unrelated_turn", "thread", "other", false},
	} {
		for _, kind := range []string{"local_started", "local_completed", "local_terminal", "failed", "interrupted", "error", "completed"} {
			t.Run(identity.name+"/"+kind, func(t *testing.T) {
				terminal := protocolv2.Turn{ID: identity.turnID, Status: protocolv2.TurnStatusCompleted, Items: []protocolv2.ThreadItem{}}
				var notification protocolv2.ServerNotification
				switch kind {
				case "local_started":
					notification = protocolv2.NewServerNotificationItemStarted(protocolv2.ServerNotificationItemStarted{Params: protocolv2.ItemStartedNotification{ThreadID: identity.threadID, TurnID: identity.turnID, Item: local}})
				case "local_completed":
					notification = protocolv2.NewServerNotificationItemCompleted(protocolv2.ServerNotificationItemCompleted{Params: protocolv2.ItemCompletedNotification{ThreadID: identity.threadID, TurnID: identity.turnID, Item: local, CompletedAtMS: 1}})
				default:
					switch kind {
					case "local_terminal":
						terminal.Items = []protocolv2.ThreadItem{local}
					case "failed":
						terminal.Status = protocolv2.TurnStatusFailed
					case "interrupted":
						terminal.Status = protocolv2.TurnStatusInterrupted
					case "error":
						terminal.Error = protocolv2.Value(protocolv2.TurnError{Message: "PRIVATE_ERROR"})
					}
					notification = protocolv2.NewServerNotificationTurnCompleted(protocolv2.ServerNotificationTurnCompleted{Params: protocolv2.TurnCompletedNotification{ThreadID: identity.threadID, Turn: terminal}})
				}
				// Reject an invalid fixture separately: a marshal failure must not make a
				// contradiction case pass without exercising typed observation.
				if _, err := json.Marshal(notification); err != nil {
					t.Fatalf("invalid typed fixture: %v", err)
				}
				evidence := Evidence{Stage: "search_turn_submitted", Turns: 2, ReplyBytes: 17}
				result := SearchEvidence{Scenario: "web", CanonicalCalls: 1, SettledTurns: 1}
				probe := searchProbe{server: &sdkProductObserver{}, threadID: "thread", turnID: "turn", previousID: "previous", scenario: "web", evidence: &evidence, result: &result, calls: map[string]map[string]string{"previous": {"credited": "unchanged"}, "turn": {}}, frames: 7, bytes: 123}
				beforeEvidence, beforeResult, beforeProbe := evidence, result, probe
				beforeCalls := map[string]map[string]string{"previous": {"credited": "unchanged"}, "turn": {}}
				err := probe.revalidate([]protocolv2.ServerNotification{notification})
				wantError := identity.correlated && kind != "completed"
				if (err != nil) != wantError {
					t.Fatalf("validation=%v, want error=%v", err, wantError)
				}
				if !reflect.DeepEqual(evidence, beforeEvidence) || !reflect.DeepEqual(result, beforeResult) || !reflect.DeepEqual(probe.calls, beforeCalls) || !reflect.DeepEqual(probe, beforeProbe) || len(probe.server.queued()) != 0 {
					t.Fatal("retained cleanup mutated evidence, result, calls, probe state, or observed queue")
				}
			})
		}
	}
}
