package oracle

import (
	"strings"

	"github.com/Harness-X-Harness/codex/grokex/validator/internal/driver"
	"github.com/Harness-X-Harness/codex/grokex/validator/internal/rollout"
)

// BasicMarker is the reply the basic scenario asks for.
const BasicMarker = "GROKEX_BASIC_RESPONSE_OK"

// BasicPrompt asks for one exact reply so the Story stays a startup and
// binding proof, not a capability proof.
const BasicPrompt = "Reply with exactly " + BasicMarker + " and no other text."

// Basic proves grokex-provider-profile-startup from the session graph and the
// delivered reply: the packaged Grokex started under the isolated Grok profile,
// bound one Thread to grok/grok-4.6, completed one Turn, persisted a non-empty
// reply, and delivered that same reply. Matching the requested marker exactly
// is recorded as a hint, not required.
func Basic(graph *rollout.Graph, run driver.TurnRun) Verdict {
	verdict, stage := newVerdict()
	verdict.Assertions["evidence_source"] = EvidenceSource
	verdict.Assertions["runner_turn_submission_count"] = 1
	verdict.Diagnostics["session_file_count"] = len(graph.Sessions)
	verdict.Diagnostics["turn_durations_seconds"] = []float64{seconds(run.Duration)}
	verdict.Diagnostics["delivered_turn_status"] = run.Status
	verdict.Diagnostics["final_response_source"] = run.FinalResponseSource

	root, ok := graph.Session(run.ThreadID)
	if !stage.require("root_session_found", ok && root.ModelProvider == driver.Provider, "root session missing or not bound to grok", "semantic_contract") {
		return *verdict
	}
	verdict.Diagnostics["root_history_mode"] = root.HistoryMode
	turn, ok := root.Turn(run.TurnID)
	if ok {
		verdict.Diagnostics["root_turn_state"] = turn.State()
		verdict.Diagnostics["root_function_calls"] = turn.FunctionCallCounts()
	}
	if !stage.require("root_turn_completed", ok && turn.Completed && !turn.Failed && run.Completed(), "Turn did not complete", deadlineOr(run, "semantic_contract")) {
		return *verdict
	}
	if !stage.require("root_turn_context_verified", turn.Model == driver.Model, "Turn context is not grok-4.6", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["provider_binding"] = driver.Provider + "/" + driver.Model
	reply := strings.TrimSpace(turn.LastAgentMessage)
	if !stage.require("reply_persisted", reply != "", "no agent reply was persisted", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["response_assertion"] = "nonempty_agent_message"
	verdict.Diagnostics["reply_matches_requested_marker"] = reply == BasicMarker
	if !stage.require("result_delivered", strings.TrimSpace(run.FinalResponse) == reply, "delivered reply differs from the canonical reply", "delivery") {
		return *verdict
	}
	verdict.Assertions["result_delivery_verified"] = true
	verdict.Assertions["status"] = "completed"
	return *verdict
}
