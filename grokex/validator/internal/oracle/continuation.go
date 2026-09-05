package oracle

import (
	"strings"

	"github.com/Harness-X-Harness/codex/grokex/validator/internal/driver"
	"github.com/Harness-X-Harness/codex/grokex/validator/internal/rollout"
)

// The continuation scenario offers one client-owned tool and asks for two
// exact replies: the first proves the tool round trip on a Grok Turn, the
// second proves the tool result travelled through canonical history into the
// next Turn.
const (
	ProbeToolName   = "grokex_live_probe"
	ProbeToolOutput = "GROKEX_LIVE_TOOL_OK"
	ProbeToolDesc   = "Return the fixed live validation marker."

	ContinuationMarker = "GROKEX_LIVE_RESPONSE_OK"
	ContinuationPrompt = "Use the " + ProbeToolName + " result, then reply " +
		"with exactly " + ContinuationMarker + " and no other text."
	HistoryMarker = ProbeToolOutput
	HistoryPrompt = "Reply with exactly the result returned by " + ProbeToolName + " in the " +
		"previous Turn and no other text. Do not call any tool."
)

// ProbeTool is the dynamic tool the driver offers and answers.
var ProbeTool = driver.DynamicTool{Name: ProbeToolName, Description: ProbeToolDesc, Output: ProbeToolOutput}

// Continuation proves grokex-encrypted-reasoning-history-continuation from the
// session graph, the answered tool requests, and the delivered replies:
//
//   - the first Turn persisted a call to the probe tool and the output the
//     validator returned, at least one reasoning item with encrypted content,
//     and completed with exactly ContinuationMarker, which was delivered;
//   - the second Turn ran on the same Thread and completed with exactly the
//     tool output, which was delivered, proving the history carried it.
func Continuation(graph *rollout.Graph, first, history driver.TurnRun, toolRequests int) Verdict {
	verdict, stage := newVerdict()
	verdict.Assertions["evidence_source"] = EvidenceSource
	verdict.Assertions["runner_turn_submission_count"] = 2
	verdict.Diagnostics["session_file_count"] = len(graph.Sessions)
	verdict.Diagnostics["turn_durations_seconds"] = []float64{seconds(first.Duration), seconds(history.Duration)}
	verdict.Diagnostics["tool_request_count"] = toolRequests
	verdict.Diagnostics["first_delivered_turn_status"] = first.Status
	verdict.Diagnostics["history_delivered_turn_status"] = history.Status
	verdict.Diagnostics["first_final_response_source"] = first.FinalResponseSource
	verdict.Diagnostics["history_final_response_source"] = history.FinalResponseSource

	root, ok := graph.Session(first.ThreadID)
	if !stage.require("root_session_found", ok && root.ModelProvider == driver.Provider, "root session missing or not bound to grok", "semantic_contract") {
		return *verdict
	}
	verdict.Diagnostics["root_history_mode"] = root.HistoryMode
	firstTurn, ok := root.Turn(first.TurnID)
	if ok {
		verdict.Diagnostics["first_turn_state"] = firstTurn.State()
		verdict.Diagnostics["first_function_calls"] = firstTurn.FunctionCallCounts()
		verdict.Diagnostics["first_encrypted_reasoning_items"] = firstTurn.EncryptedReasoningCount
	}
	if !stage.require("first_turn_completed", ok && firstTurn.Completed && !firstTurn.Failed && first.Completed(), "first Turn did not complete", deadlineOr(first, "semantic_contract")) {
		return *verdict
	}
	if !stage.require("first_turn_context_verified", firstTurn.Model == driver.Model, "first Turn context is not grok-4.6", "semantic_contract") {
		return *verdict
	}

	var probeCall rollout.FunctionCall
	found := false
	for _, call := range firstTurn.FunctionCalls {
		if call.Name == ProbeToolName {
			probeCall, found = call, true
			break
		}
	}
	if !stage.require("tool_call_persisted", found && toolRequests >= 1, "the probe tool was not called", "semantic_contract") {
		return *verdict
	}
	output, ok := firstTurn.FunctionCallOutput(probeCall.CallID)
	if !stage.require("tool_output_persisted", ok && strings.TrimSpace(output.Text) == ProbeToolOutput, "the probe output did not reach the model history", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["tool_continuation"] = "completed"
	if !stage.require("encrypted_reasoning_persisted", firstTurn.EncryptedReasoningCount >= 1, "no reasoning item with encrypted content was persisted", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["encrypted_reasoning_observed"] = true
	if !stage.require("first_reply_exact", strings.TrimSpace(firstTurn.LastAgentMessage) == ContinuationMarker, "first reply is not the requested marker", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["response_assertion"] = "exact_match"
	if !stage.require("first_result_delivered", strings.TrimSpace(first.FinalResponse) == ContinuationMarker, "delivered first reply differs from the canonical reply", "delivery") {
		return *verdict
	}

	if !stage.require("same_thread_history", history.ThreadID == first.ThreadID, "history Turn ran on another Thread", "semantic_contract") {
		return *verdict
	}
	historyTurn, ok := root.Turn(history.TurnID)
	if ok {
		verdict.Diagnostics["history_turn_state"] = historyTurn.State()
		verdict.Diagnostics["history_function_calls"] = historyTurn.FunctionCallCounts()
	}
	if !stage.require("history_turn_completed", ok && historyTurn.Completed && !historyTurn.Failed && history.Completed(), "history Turn did not complete", deadlineOr(history, "semantic_contract")) {
		return *verdict
	}
	verdict.Assertions["same_thread_history"] = "completed"
	if !stage.require("history_reply_exact", strings.TrimSpace(historyTurn.LastAgentMessage) == HistoryMarker, "history reply is not the tool output", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["history_response_assertion"] = "exact_match"
	if !stage.require("history_result_delivered", strings.TrimSpace(history.FinalResponse) == HistoryMarker, "delivered history reply differs from the canonical reply", "delivery") {
		return *verdict
	}
	verdict.Assertions["status"] = "completed"
	return *verdict
}
