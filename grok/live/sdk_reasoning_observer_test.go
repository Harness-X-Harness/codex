package live

import (
	"context"
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
	token := observer.probe.toolResult
	var price struct {
		UnitPrice int `json:"unit_price_yuan"`
	}
	decodeErr := json.Unmarshal([]byte(token), &price)
	if err != nil || decodeErr != nil || price.UnitPrice < 5 || price.UnitPrice > 19 || !reflect.DeepEqual(response, historyObserverResponse(token, true)) {
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
					protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "private_reply", Text: historyTestJSON(token), Phase: protocolv2.Value(protocolv2.MessagePhaseFinalAnswer)}),
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
				want = Evidence{Stage: "first_turn_completed", ToolCalls: 1, CompletedTools: 1, ReasoningItems: 1, FirstCompleted: true, FirstRecall: true, FirstReplyBytes: 13}
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
		historyObserverCompleted(protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "private_reply", Text: historyTestJSON(token), Phase: protocolv2.Value(protocolv2.MessagePhaseFinalAnswer)})),
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
	if !reflect.DeepEqual(observer.observations, Evidence{Stage: "turn_submitted", ToolCalls: 1, CompletedTools: 1, ReasoningItems: 1, FirstRecall: true, FirstReplyBytes: 13}) {
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
	if err != nil || !reflect.DeepEqual(denied, historyObserverResponse("Price lookup is unavailable during continuation.", false)) || !reflect.DeepEqual(DescribeFailure(failure), FailureInfo{Code: "history_tool_identity_budget_exceeded"}) || !reflect.DeepEqual(observer.observations, Evidence{ToolCalls: 4097}) {
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
	invalid := historyObserverCompleted(protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "", Text: historyTestJSON(token)}))
	// The read loop retained this item before publishing an inline response;
	// deliberately do not invoke its callback until after reconciliation.
	err := observer.reconcile(codexsdk.ThreadRunResult{Notifications: []protocolv2.ServerNotification{invalid}, Turn: protocolv2.Turn{
		ID: "private_turn", Status: protocolv2.TurnStatusCompleted, Items: []protocolv2.ThreadItem{
			protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "private_valid", Text: historyTestJSON(token)}),
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
			err := observer.typedNotification(context.Background(), historyObserverCompleted(protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "", Text: historyTestJSON(token)})))
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

func TestSDKHistoryLateFailureSettlesPreviouslyCopiedSuccessStage(t *testing.T) {
	observer, token := historyObserverFixture(t)
	observer.probe.previousID = "private_previous"
	observer.observations = Evidence{FirstCompleted: true, FirstRecall: true, FirstReplyBytes: 13, Turns: 2}
	if err := observer.reconcile(codexsdk.ThreadRunResult{Turn: protocolv2.Turn{
		ID: "private_turn", Status: protocolv2.TurnStatusCompleted, Items: []protocolv2.ThreadItem{
			protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "private_valid", Text: historyTestJSON(token, 3)}),
		},
	}}); err != nil {
		t.Fatal(err)
	}
	observer.finishTurn()
	observer.observations.Stage = "history_recalled"
	copied := observer.observations
	// Force the exact schedule: inline proof and returned evidence copy precede
	// a semantically invalid callback admitted before SDK cleanup joins it.
	late := historyObserverCompleted(protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "", Text: historyTestJSON(token, 3)}))
	if err := observer.typedNotification(context.Background(), late); DescribeFailure(err).Code != "assistant_identity_missing" {
		t.Fatal("late invalid item was not rejected")
	}
	want := copied
	want.Stage = "continuation_completed"
	got := settleSDKHistoryEvidence(copied, observer.failure(nil))
	if !reflect.DeepEqual(got, want) || !got.FirstCompleted || !got.FirstRecall || !got.Completed || !got.Recalled {
		t.Fatal("late rejection retained a success stage or erased prior facts")
	}
	if got := settleSDKHistoryEvidence(copied, nil); !reflect.DeepEqual(got, copied) {
		t.Fatal("successful joined observation lost its success stage")
	}
	pending := copied
	pending.CleanupPending = true
	want.CleanupPending = true
	if got := settleSDKHistoryEvidence(pending, context.DeadlineExceeded); !reflect.DeepEqual(got, want) {
		t.Fatal("unjoined cleanup failure retained the whole-scenario success stage")
	}
}

// Exact capacity is exercised through the real typed observer and production
// continuation reset, without making SDK process throughput the acceptance goal.
func TestSDKHistoryObserverAssistantCapacity(t *testing.T) {
	for _, mode := range []string{"limit_and_duplicate", "summary_recall", "summary_no_recall", "overflow"} {
		t.Run(mode, func(t *testing.T) {
			observer, token := historyObserverFixture(t)
			for turn := 0; turn < 2; turn++ {
				turnID := "private_turn"
				if turn == 1 {
					if observer.beginContinuation() != "private_thread" || observer.probe.previousID != turnID ||
						len(observer.reply.identities) != 0 || observer.reply.completed || observer.reply.authoritative || observer.reply.explicitFinal || observer.reply.recall ||
						observer.reconciled || observer.callbackTerminalSeen || observer.deliveredTyped != 0 || observer.recoveredTyped != 0 {
						t.Fatal("production continuation transition retained prior reply credit or identity budget")
					}
					turnID = "private_next"
					observer.active = true // The real Resume admission owns this flag.
				}
				if err := observer.reconcile(codexsdk.ThreadRunResult{Turn: protocolv2.Turn{ID: turnID, Status: protocolv2.TurnStatusInProgress}}); err != nil {
					t.Fatal(err)
				}
				item := func(id, text string) protocolv2.ThreadItem {
					return protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: id, Text: historyTestJSON(text, turn+2), Phase: protocolv2.Value(protocolv2.MessagePhaseFinalAnswer)})
				}
				send := func(id, text string) error {
					return observer.typedNotification(context.Background(), protocolv2.NewServerNotificationItemCompleted(protocolv2.ServerNotificationItemCompleted{Params: protocolv2.ItemCompletedNotification{
						ThreadID: "private_thread", TurnID: turnID, CompletedAtMS: 1, Item: item(id, text),
					}}))
				}
				for i := 0; i < historyAssistantLimit; i++ {
					text := ""
					if (mode == "limit_and_duplicate" || mode == "overflow") && i == historyAssistantLimit-1 {
						text = token
					}
					if err := send("private_item_"+strconv.Itoa(i), text); err != nil {
						t.Fatal(err)
					}
				}
				if len(observer.reply.identities) != historyAssistantLimit {
					t.Fatal("exact identity capacity was not retained")
				}
				if mode == "limit_and_duplicate" || mode == "overflow" {
					if err := send("private_item_0", ""); err != nil {
						t.Fatal(err)
					}
					if observer.reply.evaluated || observer.reply.recall || observer.reply.ambiguous || len(observer.reply.identities) != historyAssistantLimit || observer.reply.text.String() != historyTestJSON(token, turn+2) {
						t.Fatal("old duplicate spent capacity or superseded the current result")
					}
				}
				if mode == "overflow" && turn == 1 {
					err := send("private_overflow", token)
					observer.finishTurn()
					if DescribeFailure(err).Code != "assistant_identity_budget_exceeded" || len(observer.reply.identities) != historyAssistantLimit || observer.observations.Completed || !observer.observations.FirstCompleted || !observer.observations.FirstRecall {
						t.Fatal("capacity + 1 was not rejected before completion or damaged prior-turn proof")
					}
					encoded, _ := json.Marshal(struct {
						Evidence Evidence
						Failure  FailureInfo
						Error    string
					}{observer.observations, DescribeFailure(err), err.Error()})
					if strings.Contains(string(encoded), "private_") || strings.Contains(string(encoded), token) {
						t.Fatal("capacity rejection exported private proof data")
					}
					continue
				}
				items := []protocolv2.ThreadItem{}
				wantRecall := true
				if strings.HasPrefix(mode, "summary_") {
					text := token
					if mode == "summary_no_recall" && turn == 1 {
						text, wantRecall = "wrong", false
					}
					items = append(items, item("new-summary-id", text))
				}
				err := observer.typedNotification(context.Background(), protocolv2.NewServerNotificationTurnCompleted(protocolv2.ServerNotificationTurnCompleted{Params: protocolv2.TurnCompletedNotification{
					ThreadID: "private_thread", Turn: protocolv2.Turn{ID: turnID, Status: protocolv2.TurnStatusCompleted, Items: items},
				}}))
				observer.finishTurn()
				if (err == nil) != wantRecall || !observer.reply.completed || observer.reply.recall != wantRecall || observer.reply.ambiguous {
					t.Fatal("terminal authority or capacity result changed")
				}
				if !wantRecall && DescribeFailure(err).Code != "history_reply_invalid" {
					t.Fatal("wrong authoritative summary was not rejected as missing recall")
				}
				if strings.HasPrefix(mode, "summary_") && !observer.reply.authoritative {
					t.Fatal("terminal summary at capacity lost authority")
				}
			}
		})
	}
}
