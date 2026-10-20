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

func TestSDKHistoryProjectionNullableEvidence(t *testing.T) {
	for _, tc := range []struct {
		name      string
		phase     *protocolv2.Nullable[protocolv2.MessagePhase]
		namespace *protocolv2.Nullable[string]
		success   *protocolv2.Nullable[bool]
		recall    bool
		tool      bool
	}{
		{name: "omitted", recall: true},
		{name: "null", phase: protocolv2.Null[protocolv2.MessagePhase](), namespace: protocolv2.Null[string](), success: protocolv2.Null[bool](), recall: true},
		{name: "final", phase: protocolv2.Value(protocolv2.MessagePhaseFinalAnswer), success: protocolv2.Value(true), recall: true, tool: true},
		{name: "commentary", phase: protocolv2.Value(protocolv2.MessagePhaseCommentary), success: protocolv2.Value(true), tool: true},
		{name: "empty_namespace", namespace: protocolv2.Value(""), success: protocolv2.Value(true), recall: true},
		{name: "failed_tool", success: protocolv2.Value(false), recall: true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			observer, token := historyObserverFixture(t)
			if err := observer.reconcile(codexsdk.ThreadRunResult{Turn: protocolv2.Turn{ID: "private_turn", Status: protocolv2.TurnStatusInProgress}}); err != nil {
				t.Fatal(err)
			}
			tool, _ := historyObserverTool("private_call_0", token).AsDynamicToolCall()
			tool.Namespace, tool.Success = tc.namespace, tc.success
			tool.ContentItems = protocolv2.Value([]protocolv2.DynamicToolCallOutputContentItem{
				protocolv2.NewDynamicToolCallOutputContentItemInputImage(protocolv2.DynamicToolCallOutputContentItemInputImage{ImageURL: token}),
				protocolv2.NewDynamicToolCallOutputContentItemInputText(protocolv2.DynamicToolCallOutputContentItemInputText{Text: "回答: " + token}),
			})
			for _, value := range []protocolv2.ThreadItem{
				protocolv2.NewThreadItemDynamicToolCall(tool),
				protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "private_reply", Phase: tc.phase, Text: "回答: " + token}),
			} {
				if err := observer.typedNotification(context.Background(), historyObserverCompleted(value)); err != nil {
					t.Fatal(err)
				}
			}
			observer.finishTurn()
			wantTools := 0
			if tc.tool {
				wantTools = 1
			}
			if observer.observations.CompletedTools != wantTools || observer.observations.FirstRecall != tc.recall || observer.observations.FirstReplyBytes != len("回答: "+token) || observer.observations.FirstCompleted {
				t.Fatalf("nullable projection changed evidence: %+v", observer.observations)
			}
		})
	}
}

func TestSDKHistoryTypedEarlyEnvelopeBudget(t *testing.T) {
	// Reasoning content is not retained by the evidence projection, but every
	// encoded byte (including JSON escaping and notification metadata) counts.
	content := []string{strings.Repeat("unused<&", 1024)}
	notification := historyObserverCompleted(protocolv2.NewThreadItemReasoning(protocolv2.ThreadItemReasoning{ID: "private_reasoning", Content: &content}))
	emittedAt := int64(123)
	notification.EmittedAtMS = &emittedAt
	raw, err := json.Marshal(notification)
	if err != nil {
		t.Fatal(err)
	}
	for _, extra := range []int{-1, 0, 1} {
		t.Run(strconv.Itoa(extra), func(t *testing.T) {
			observer, _ := historyObserverFixture(t)
			observer.pendingBytes = (8 << 20) - len(raw) + extra
			err := observer.typedNotification(context.Background(), notification)
			if extra > 0 {
				if DescribeFailure(err).Code != "early_evidence_budget_exceeded" || len(observer.pending) != 0 {
					t.Fatal("oversized typed envelope gained pending evidence")
				}
			} else if err != nil || len(observer.pending) != 1 || observer.pendingBytes != (8<<20)+extra {
				t.Fatal("in-budget typed envelope did not use its exact encoded size")
			}
		})
	}
	observer, _ := historyObserverFixture(t)
	observer.pending = make([]sdkHistoryObservation, 8191)
	if err := observer.typedNotification(context.Background(), notification); err != nil || len(observer.pending) != 8192 {
		t.Fatal("last allowed pending observation was rejected")
	}
	if err := observer.typedNotification(context.Background(), notification); DescribeFailure(err).Code != "early_evidence_budget_exceeded" {
		t.Fatal("pending observation count limit was weakened")
	}
}

func TestSDKHistoryProjectedTerminalDiagnostics(t *testing.T) {
	for _, oversized := range []bool{false, true} {
		for _, source := range []string{"callback", "retained", "inline"} {
			t.Run(source+"/"+strconv.FormatBool(oversized), func(t *testing.T) {
				observer, _ := historyObserverFixture(t)
				message := "PRIVATE_PROVIDER_DETAIL"
				kind := "httpConnectionFailed"
				status := 429
				if oversized {
					message = strings.Repeat(message, 1024)
					kind, status = "unknown", 0
				}
				terminal := protocolv2.Turn{ID: "private_turn", Status: protocolv2.TurnStatusFailed, Items: []protocolv2.ThreadItem{}, Error: protocolv2.Value(protocolv2.TurnError{
					Message: message, CodexErrorInfo: protocolv2.Value(protocolv2.NewCodexErrorInfoHTTPConnectionFailed(protocolv2.CodexErrorInfoHTTPConnectionFailed{HTTPStatusCode: protocolv2.Value(uint16(429))})),
				})}
				notification := protocolv2.NewServerNotificationTurnCompleted(protocolv2.ServerNotificationTurnCompleted{Params: protocolv2.TurnCompletedNotification{ThreadID: "private_thread", Turn: terminal}})
				var err error
				if source == "inline" {
					err = observer.reconcile(codexsdk.ThreadRunResult{Turn: terminal})
				} else {
					if err = observer.reconcile(codexsdk.ThreadRunResult{Turn: protocolv2.Turn{ID: "private_turn", Status: protocolv2.TurnStatusInProgress}}); err != nil {
						t.Fatal(err)
					}
					if source == "callback" {
						err = observer.typedNotification(context.Background(), notification)
					} else {
						observer.recoverNotifications([]protocolv2.ServerNotification{notification})
						err = observer.cause
					}
				}
				want := FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: kind, HTTPStatus: status}
				if !reflect.DeepEqual(DescribeFailure(err), want) || observer.reply.completed || strings.Contains(err.Error(), "PRIVATE_") {
					t.Fatalf("terminal projection changed bounded diagnostics: %+v", DescribeFailure(err))
				}
			})
		}
	}
}
