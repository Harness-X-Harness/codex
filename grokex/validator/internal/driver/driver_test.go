package driver

import (
	"context"
	"errors"
	"reflect"
	"testing"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func agentMessage(id, text string) protocolv2.ThreadItem {
	return protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: id, Text: text})
}

func TestLastAgentMessageIgnoresPhaseAndEmptyItems(t *testing.T) {
	items := []protocolv2.ThreadItem{
		agentMessage("m1", "first"),
		protocolv2.NewThreadItemCommandExecution(protocolv2.ThreadItemCommandExecution{ID: "c1"}),
		agentMessage("m2", "3f2b1c9e-8d4a-4b6f-9a1e-7c5d2e8f0a11"),
		agentMessage("m3", ""),
	}
	if got := lastAgentMessage(items); got != "3f2b1c9e-8d4a-4b6f-9a1e-7c5d2e8f0a11" {
		t.Fatalf("lastAgentMessage = %q", got)
	}
	if got := lastAgentMessage(nil); got != "" {
		t.Fatalf("lastAgentMessage(nil) = %q", got)
	}
}

func TestClassifyTreatsPhaselessCompletedTurnAsCompleted(t *testing.T) {
	completed := TurnRun{Status: string(protocolv2.TurnStatusCompleted)}
	phaseless := errors.New("codexsdk: turn completed without final_answer agent message")
	if err := classify(completed, phaseless); err != nil {
		t.Fatalf("phaseless completed turn classified as %v", err)
	}
	if err := classify(TurnRun{Status: "inProgress"}, phaseless); err == nil {
		t.Fatal("phaseless error on a non-completed turn must stay an error")
	}
	if err := classify(TurnRun{}, context.DeadlineExceeded); !errors.Is(err, ErrDeadline) {
		t.Fatalf("deadline classified as %v", err)
	}
	if err := classify(completed, nil); err != nil {
		t.Fatalf("nil classified as %v", err)
	}
}

func TestServerRequestsAnswerOnlyTheConfiguredTool(t *testing.T) {
	requests := NewServerRequests(&DynamicTool{Name: "grokex_live_probe", Output: "GROKEX_LIVE_TOOL_OK"})
	call := protocolv2.NewServerRequestItemToolCall(protocolv2.ServerRequestItemToolCall{
		Params: protocolv2.DynamicToolCallParams{
			Tool:      "grokex_live_probe",
			CallID:    "probe-1",
			Arguments: protocolv2.JSONObject(map[string]protocolv2.JSONValue{}),
		},
	})
	if _, err := requests.Handler(context.Background(), call); err != nil {
		t.Fatalf("probe call: %v", err)
	}
	other := protocolv2.NewServerRequestItemToolCall(protocolv2.ServerRequestItemToolCall{
		Params: protocolv2.DynamicToolCallParams{Tool: "something_else", CallID: "x", Arguments: protocolv2.JSONObject(map[string]protocolv2.JSONValue{})},
	})
	if _, err := requests.Handler(context.Background(), other); err == nil {
		t.Fatal("unexpected tool must fail closed")
	}
	approval := protocolv2.NewServerRequestItemCommandExecutionRequestApproval(protocolv2.ServerRequestItemCommandExecutionRequestApproval{})
	if _, err := requests.Handler(context.Background(), approval); err != nil {
		t.Fatalf("approval must be declined, not failed: %v", err)
	}
	if requests.ToolRequestCount() != 1 {
		t.Fatalf("tool request count = %d", requests.ToolRequestCount())
	}
	diagnostics := requests.Diagnostics()
	if !reflect.DeepEqual(diagnostics["server_requests_by_kind"], map[string]int{"item/tool/call": 2, "item/commandExecution/requestApproval": 1}) {
		t.Fatalf("by kind = %#v", diagnostics["server_requests_by_kind"])
	}
	if !reflect.DeepEqual(diagnostics["unexpected_tool_request_names"], []string{"something_else"}) {
		t.Fatalf("unexpected names = %#v", diagnostics["unexpected_tool_request_names"])
	}
}

func TestDynamicToolSpecIsAnEmptyObjectFunction(t *testing.T) {
	spec := DynamicTool{Name: "grokex_live_probe", Description: "d"}.Spec()
	function, ok := spec.AsFunction()
	if !ok || function.Name != "grokex_live_probe" || function.Description != "d" {
		t.Fatalf("spec = %+v ok=%v", function, ok)
	}
	schema, ok := function.InputSchema.AsObject()
	if !ok {
		t.Fatal("schema is not an object")
	}
	if kind, _ := schema["type"].AsString(); kind != "object" {
		t.Fatalf("schema type = %q", kind)
	}
}
