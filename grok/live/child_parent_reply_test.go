package live

import (
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func TestChildParentTerminalReply(t *testing.T) {
	const result = testInvoice
	message := func(id, text string) protocolv2.ThreadItem {
		return protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: id, Text: text})
	}
	item := func(raw string) protocolv2.ThreadItem {
		var value protocolv2.ThreadItem
		if err := json.Unmarshal([]byte(raw), &value); err != nil {
			t.Fatal(err)
		}
		return value
	}
	wait := item(`{"type":"collabAgentToolCall","id":"wait","tool":"wait","status":"completed","senderThreadId":"parent","receiverThreadIds":[],"agentsStates":{}}`)
	spawn := item(`{"type":"collabAgentToolCall","id":"spawn","tool":"spawnAgent","status":"completed","senderThreadId":"parent","receiverThreadIds":["child"],"agentsStates":{}}`)
	activity := item(`{"type":"subAgentActivity","id":"activity","kind":"completed","agentThreadId":"child","agentPath":"/root/live_child"}`)
	v2spawn := item(`{"type":"subAgentActivity","id":"spawn-call","kind":"started","agentThreadId":"child","agentPath":"/root/live_child"}`)
	failedWait := item(`{"type":"collabAgentToolCall","id":"failed-wait","tool":"wait","status":"failed","senderThreadId":"parent","receiverThreadIds":[],"agentsStates":{}}`)
	pendingWait := item(`{"type":"collabAgentToolCall","id":"pending-wait","tool":"wait","status":"inProgress","senderThreadId":"parent","receiverThreadIds":[],"agentsStates":{}}`)
	unboundStart := item(`{"type":"subAgentActivity","id":"other","kind":"started","agentThreadId":"unrelated","agentPath":"/root/other"}`)
	good := message("final", result)
	for _, tc := range []struct {
		name               string
		items              []protocolv2.ThreadItem
		reply              string
		extractOK, parseOK bool
	}{
		{"v2_spawn_without_wait", []protocolv2.ThreadItem{message("before_spawn", `Checking the invoice.`), v2spawn, good}, result, true, true},
		{"failed_tool_recovery", []protocolv2.ThreadItem{message("before", "pending"), failedWait, good}, result, true, true},
		{"pending_tool_not_boundary", []protocolv2.ThreadItem{message("before", "pending"), pendingWait, good}, "pending\n" + result, true, false},
		{"actual_tool_rounds", []protocolv2.ThreadItem{message("before_spawn", `Checking the invoice.`), spawn, message("before_wait", `Checking the invoice.`), wait, good}, result, true, true},
		{"large_intermediate", []protocolv2.ThreadItem{message("large", strings.Repeat("x", 5000)), wait, good}, result, true, true},
		{"unbound_started_after_final", []protocolv2.ThreadItem{wait, good, unboundStart}, result, true, true},
		{"activity_after_final", []protocolv2.ThreadItem{wait, good, activity}, result, true, true},
		{"activity_does_not_reset", []protocolv2.ThreadItem{wait, message("invalid", "not json"), activity, good}, "not json\n" + result, true, false},
		{"posttool_invalid_no_fallback", []protocolv2.ThreadItem{good, wait, message("invalid", "not json")}, "not json", true, false},
		{"posttool_null_no_fallback", []protocolv2.ThreadItem{good, wait, message("null", `Checking the invoice.`)}, `Checking the invoice.`, true, false},
		{"posttool_multiple_no_fallback", []protocolv2.ThreadItem{wait, good, message("second", result)}, result + "\n" + result, true, false},
		{"posttool_absent_no_fallback", []protocolv2.ThreadItem{good, wait}, "", false, false},
		{"intermediate_identity_conflict", []protocolv2.ThreadItem{message("same", "a"), message("same", "b"), wait, good}, "", false, false},
		{"cross_boundary_identity_conflict", []protocolv2.ThreadItem{message("same", "a"), wait, message("same", result)}, "", false, false},
		{"large_final_is_fully_validated", []protocolv2.ThreadItem{wait, message("large", strings.Repeat("x", 4097))}, strings.Repeat("x", 4097), true, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			reply, err := childParentTerminalReply(productTurn{Items: tc.items}, map[string]bool{"child": true})
			if (err == nil) != tc.extractOK || err == nil && reply != tc.reply {
				t.Fatalf("terminal extraction: reply=%q err=%v", reply, err)
			}
			if err != nil {
				return
			}
			if validChildInvoice(reply) != tc.parseOK {
				t.Fatalf("invoice outcome: %q", reply)
			}
		})
	}
}

func TestChildTerminalSelectionWithoutPresentationCaps(t *testing.T) {
	path := "/root/" + strings.Repeat("a", 1100)
	activity := protocolv2.NewThreadItemSubAgentActivity(protocolv2.ThreadItemSubAgentActivity{ID: "spawn", Kind: "started", AgentThreadID: "child", AgentPath: path})
	items := make([]protocolv2.ThreadItem, 0, 132)
	for i := 0; i < 129; i++ {
		items = append(items, invoiceMessage(fmt.Sprintf("progress-%d", i), "", "Checking."))
	}
	items = append(items, activity, invoiceMessage("answer", "", testInvoice))
	turn := productTurn{Items: items}
	candidates, err := childCandidates(turn, "parent", nil)
	if err != nil || candidates["child"] != path {
		t.Fatal("valid native path was hidden by a Live string-length rule")
	}
	reply, err := childParentTerminalReply(turn, map[string]bool{"child": true})
	if err != nil || reply != testInvoice {
		t.Fatal("irrelevant earlier items or long bound path erased the terminal invoice")
	}
}
