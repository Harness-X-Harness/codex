package live

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"reflect"
	"strconv"
	"strings"
	"testing"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func historyObserverRequest(callID string) protocolv2.ServerRequest {
	return protocolv2.NewServerRequestItemToolCall(protocolv2.ServerRequestItemToolCall{
		ID: protocolv2.NewRequestIdInt64(1), Params: protocolv2.DynamicToolCallParams{
			ThreadID: "private_thread", TurnID: "private_turn", CallID: callID, Tool: historyTool, Arguments: protocolv2.JSONObject(map[string]protocolv2.JSONValue{}),
		},
	})
}

func historyObserverResponse(text string, success bool) codexsdk.ServerRequestResponse {
	return codexsdk.DynamicToolResponse(protocolv2.DynamicToolCallResponse{
		Success: success, ContentItems: []protocolv2.DynamicToolCallOutputContentItem{protocolv2.NewDynamicToolCallOutputContentItemInputText(protocolv2.DynamicToolCallOutputContentItemInputText{Text: text})},
	})
}

func historyObserverFixture(t *testing.T) (*sdkHistoryObserver, string) {
	t.Helper()
	observer := newSDKHistoryObserver()
	observer.active, observer.probe.threadID = true, "private_thread"
	response, err := observer.request(context.Background(), historyObserverRequest("private_call_0"))
	token := observer.probe.token
	decoded, decodeErr := hex.DecodeString(token)
	if err != nil || decodeErr != nil || len(decoded) != 32 || !reflect.DeepEqual(response, historyObserverResponse(token, true)) {
		t.Fatal("authorized provisional request did not return a fresh tool result")
	}
	return observer, token
}

func historyObserverTool(callID, token string) protocolv2.ThreadItem {
	return protocolv2.NewThreadItemDynamicToolCall(protocolv2.ThreadItemDynamicToolCall{
		ID: callID, Tool: historyTool, Arguments: protocolv2.JSONObject(map[string]protocolv2.JSONValue{}), Status: protocolv2.DynamicToolCallStatusCompleted, Success: protocolv2.Value(true),
		ContentItems: protocolv2.Value([]protocolv2.DynamicToolCallOutputContentItem{protocolv2.NewDynamicToolCallOutputContentItemInputText(protocolv2.DynamicToolCallOutputContentItemInputText{Text: token})}),
	})
}

func historyObserverCompleted(item protocolv2.ThreadItem) protocolv2.ServerNotification {
	return protocolv2.NewServerNotificationItemCompleted(protocolv2.ServerNotificationItemCompleted{
		Params: protocolv2.ItemCompletedNotification{ThreadID: "private_thread", TurnID: "private_turn", CompletedAtMS: 1, Item: item},
	})
}

func TestSDKHistoryObserverProvisionalReconciliation(t *testing.T) {
	for label, actualID := range map[string]string{"matching": "private_turn", "mismatched": "private_other_turn", "missing": ""} {
		t.Run(label, func(t *testing.T) {
			observer, token := historyObserverFixture(t)
			tool := historyObserverCompleted(historyObserverTool("private_call_0", token))
			terminal := protocolv2.NewServerNotificationTurnCompleted(protocolv2.ServerNotificationTurnCompleted{Params: protocolv2.TurnCompletedNotification{
				ThreadID: "private_thread", Turn: protocolv2.Turn{ID: "private_turn", Status: protocolv2.TurnStatusCompleted, Items: []protocolv2.ThreadItem{
					protocolv2.NewThreadItemReasoning(protocolv2.ThreadItemReasoning{ID: "private_reasoning"}),
					protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "private_reply", Text: token, Phase: protocolv2.Value(protocolv2.MessagePhaseFinalAnswer)}),
				}},
			}})
			for _, notification := range []protocolv2.ServerNotification{tool, terminal} {
				if err := observer.typedNotification(context.Background(), notification); err != nil {
					t.Fatal("valid early callback was rejected")
				}
			}
			observer.recoverNotifications([]protocolv2.ServerNotification{tool, terminal})
			if !reflect.DeepEqual(observer.observations, Evidence{ToolCalls: 1}) {
				t.Fatal("provisional identity credited completion before actual turn admission")
			}
			err := observer.reconcile(codexsdk.ThreadRunResult{Turn: protocolv2.Turn{ID: actualID, Status: protocolv2.TurnStatusInProgress}, Notifications: []protocolv2.ServerNotification{terminal}})
			observer.recoverNotifications([]protocolv2.ServerNotification{tool, terminal})
			observer.finishTurn()
			want := Evidence{ToolCalls: 1}
			if actualID == "private_turn" {
				want = Evidence{Stage: "first_turn_completed", ToolCalls: 1, CompletedTools: 1, ReasoningItems: 1, FirstCompleted: true, FirstRecall: true, FirstReplyBytes: 64}
			} else if DescribeFailure(err).Code != "turn_identity_missing" {
				t.Fatal("mismatched actual turn did not reject provisional authority")
			}
			if actualID == "private_turn" && err != nil || !reflect.DeepEqual(observer.observations, want) {
				t.Fatal("actual turn reconciliation lost or falsely credited causal evidence")
			}
		})
	}
}

func TestSDKHistoryObserverCompletedToolAuthority(t *testing.T) {
	observer, token := historyObserverFixture(t)
	if err := observer.reconcile(codexsdk.ThreadRunResult{Turn: protocolv2.Turn{ID: "private_turn", Status: protocolv2.TurnStatusInProgress}}); err != nil {
		t.Fatal("valid turn was rejected")
	}
	for index, callID := range []string{"private_forged_call", "private_call_0"} {
		if err := observer.typedNotification(context.Background(), historyObserverCompleted(historyObserverTool(callID, token))); err != nil || !reflect.DeepEqual(observer.observations, Evidence{Stage: "turn_submitted", ToolCalls: 1, CompletedTools: index}) {
			t.Fatal("completed tool result was not correlated with an accepted request")
		}
	}
}

func TestSDKHistoryObserverFailurePrefixRecovery(t *testing.T) {
	observer, token := historyObserverFixture(t)
	if err := observer.reconcile(codexsdk.ThreadRunResult{Turn: protocolv2.Turn{ID: "private_turn", Status: protocolv2.TurnStatusInProgress}}); err != nil {
		t.Fatal("valid turn was rejected")
	}
	notifications := []protocolv2.ServerNotification{
		historyObserverCompleted(protocolv2.NewThreadItemReasoning(protocolv2.ThreadItemReasoning{ID: "private_reasoning"})),
		historyObserverCompleted(historyObserverTool("private_call_0", token)),
		historyObserverCompleted(protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "private_reply", Text: token, Phase: protocolv2.Value(protocolv2.MessagePhaseFinalAnswer)})),
	}
	if err := observer.typedNotification(context.Background(), notifications[0]); err != nil {
		t.Fatal("valid delivered prefix was rejected")
	}
	observer.recoverNotifications(notifications)
	for _, notification := range notifications[1:] {
		if err := observer.typedNotification(context.Background(), notification); err != nil {
			t.Fatal("delayed recovered callback was rejected")
		}
	}
	observer.recoverNotifications(notifications)
	observer.finishTurn()
	if !reflect.DeepEqual(observer.observations, Evidence{Stage: "turn_submitted", ToolCalls: 1, CompletedTools: 1, ReasoningItems: 1, FirstRecall: true, FirstReplyBytes: 64}) {
		t.Fatal("failure recovery duplicated partial evidence or invented terminal completion")
	}
}

func TestSDKHistoryObserverCallCapacity(t *testing.T) {
	observer, token := historyObserverFixture(t)
	for index := 1; index <= historyToolIdentityLimit; index++ {
		callID := "private_call_" + strconv.Itoa(index)
		if index == historyToolIdentityLimit {
			callID = "private_call_0"
		}
		response, err := observer.request(context.Background(), historyObserverRequest(callID))
		if err != nil || !reflect.DeepEqual(response, historyObserverResponse(token, true)) {
			t.Fatal("in-budget request or duplicate at capacity lost its authorized result")
		}
	}
	denied, err := observer.request(context.Background(), historyObserverRequest("private_new_call"))
	failure := observer.failure(context.DeadlineExceeded)
	observer.finishTurn()
	if err != nil || !reflect.DeepEqual(denied, historyObserverResponse("History probe is unavailable during continuation.", false)) || !reflect.DeepEqual(DescribeFailure(failure), FailureInfo{Code: "history_tool_identity_budget_exceeded"}) || !reflect.DeepEqual(observer.observations, Evidence{ToolCalls: 4097}) {
		t.Fatal("new identity at capacity was not safely denied without false completion")
	}
	safe, marshalErr := json.Marshal(struct {
		Evidence Evidence
		Failure  FailureInfo
		Error    string
	}{observer.observations, DescribeFailure(failure), failure.Error()})
	if marshalErr != nil || strings.Contains(string(safe), token) || strings.Contains(string(safe), "private_") {
		t.Fatal("capacity denial exported private causal evidence")
	}
}

func TestSDKHistoryRetainedInvalidItemPrecedesInlineCredit(t *testing.T) {
	observer, token := historyObserverFixture(t)
	invalid := historyObserverCompleted(protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "", Text: token}))
	// The read loop retained this item before publishing an inline response;
	// deliberately do not invoke its callback until after reconciliation.
	err := observer.reconcile(codexsdk.ThreadRunResult{Notifications: []protocolv2.ServerNotification{invalid}, Turn: protocolv2.Turn{
		ID: "private_turn", Status: protocolv2.TurnStatusCompleted, Items: []protocolv2.ThreadItem{
			protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "private_valid", Text: token}),
		},
	}})
	if DescribeFailure(err).Code != "assistant_identity_missing" || observer.reply.completed {
		t.Fatal("inline response bypassed an admitted invalid item")
	}
	_ = observer.typedNotification(context.Background(), invalid)
	if DescribeFailure(observer.cause).Code != "assistant_identity_missing" {
		t.Fatal("delayed callback cleared rejection")
	}
}

func TestSDKHistoryFinishedTurnRetainsContradictionChecks(t *testing.T) {
	for _, prior := range []bool{false, true} {
		t.Run(strconv.FormatBool(prior), func(t *testing.T) {
			observer, token := historyObserverFixture(t)
			if err := observer.reconcile(codexsdk.ThreadRunResult{Turn: protocolv2.Turn{ID: "private_turn", Status: protocolv2.TurnStatusInProgress}}); err != nil {
				t.Fatal(err)
			}
			observer.finishTurn()
			if prior {
				observer.active = true
				observer.probe.previousID, observer.probe.turnID = "private_turn", "private_next"
			}
			before := observer.observations
			err := observer.typedNotification(context.Background(), historyObserverCompleted(protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "", Text: token})))
			if DescribeFailure(err).Code != "assistant_identity_missing" || !reflect.DeepEqual(before, observer.observations) {
				t.Fatal("late contradiction lost or positive evidence changed")
			}
		})
	}
}

func TestSDKHistoryFinishedTurnIgnoresUnrelatedInvalidItem(t *testing.T) {
	for _, field := range []string{"thread", "turn"} {
		t.Run(field, func(t *testing.T) {
			observer, _ := historyObserverFixture(t)
			if err := observer.reconcile(codexsdk.ThreadRunResult{Turn: protocolv2.Turn{ID: "private_turn", Status: protocolv2.TurnStatusInProgress}}); err != nil {
				t.Fatal(err)
			}
			observer.finishTurn()
			params := protocolv2.ItemCompletedNotification{ThreadID: "private_thread", TurnID: "private_turn", Item: protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "", Text: "irrelevant"})}
			if field == "thread" {
				params.ThreadID = "private_other"
			} else {
				params.TurnID = "private_other"
			}
			before := observer.observations
			if err := observer.typedNotification(context.Background(), protocolv2.NewServerNotificationItemCompleted(protocolv2.ServerNotificationItemCompleted{Params: params})); err != nil || !reflect.DeepEqual(before, observer.observations) {
				t.Fatal("unrelated item changed settled proof")
			}
		})
	}
}
