package oracle

import (
	"os"
	"path/filepath"
	"strings"

	"github.com/Harness-X-Harness/codex/grokex/validator/internal/driver"
	"github.com/Harness-X-Harness/codex/grokex/validator/internal/rollout"
)

// ApplyPatchMarker is the reply the custom-apply-patch scenario asks for.
const ApplyPatchMarker = "GROKEX_APPLY_PATCH_OK"

// ApplyPatchPrompt asks for one file edit through custom apply_patch so a
// shell or apply-patch CLI path cannot complete the Story.
const ApplyPatchPrompt = "Replace the exact contents of hello.txt from HELLO to WORLD using apply_patch. " +
	"Do not use a shell or exec_command. When the file contains WORLD, reply with exactly " +
	ApplyPatchMarker + " and no other text."

const (
	applyPatchFile     = "hello.txt"
	applyPatchSeed     = "HELLO\n"
	applyPatchExpected = "WORLD"
)

// SeedApplyPatchWorkspace writes the seeded file the proof Turn must edit.
func SeedApplyPatchWorkspace(workspace string) error {
	return os.WriteFile(filepath.Join(workspace, applyPatchFile), []byte(applyPatchSeed), 0o644)
}

// ApplyPatch proves grokex-custom-apply-patch from the session graph, the
// delivered reply, and the workspace file: the packaged Grokex bound one
// Thread to grok/grok-4.6, completed one Turn, persisted custom apply_patch
// or an equivalent file_change, left no shell edit path, and the workspace
// file shows the expected result.
func ApplyPatch(graph *rollout.Graph, run driver.TurnRun, workspace string) Verdict {
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
		verdict.Diagnostics["file_change_count"] = turn.FileChangeCount
		verdict.Diagnostics["command_execution_count"] = turn.CommandExecutionCount
	}
	if !stage.require("root_turn_completed", ok && turn.Completed && !turn.Failed && run.Completed(), "Turn did not complete", deadlineOr(run, "semantic_contract")) {
		return *verdict
	}
	if !stage.require("root_turn_context_verified", turn.Model == driver.Model, "Turn context is not grok-4.6", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["provider_binding"] = driver.Provider + "/" + driver.Model

	if !stage.require("apply_patch_path_verified", hasApplyPatchEvidence(turn), "custom apply_patch or equivalent file_change is missing", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["apply_patch_path"] = "custom_apply_patch"

	if !stage.require("shell_edit_absent", !hasShellEditEvidence(turn), "a shell or command_execution path explained the edit", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["shell_edit_absent"] = true

	contents, err := os.ReadFile(filepath.Join(workspace, applyPatchFile))
	fileOK := err == nil && strings.TrimSpace(string(contents)) == applyPatchExpected
	verdict.Diagnostics["workspace_file_readable"] = err == nil
	if !stage.require("workspace_file_verified", fileOK, "workspace file does not show the expected result", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["workspace_file_result"] = applyPatchExpected

	reply := strings.TrimSpace(turn.LastAgentMessage)
	if !stage.require("reply_persisted", reply != "", "no agent reply was persisted", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["response_assertion"] = "nonempty_agent_message"
	verdict.Diagnostics["reply_matches_requested_marker"] = reply == ApplyPatchMarker
	if !stage.require("result_delivered", strings.TrimSpace(run.FinalResponse) == reply, "delivered reply differs from the canonical reply", "delivery") {
		return *verdict
	}
	verdict.Assertions["result_delivery_verified"] = true
	verdict.Assertions["status"] = "completed"
	return *verdict
}

func hasApplyPatchEvidence(turn *rollout.Turn) bool {
	for _, call := range turn.FunctionCalls {
		if isApplyPatchCall(call) {
			return true
		}
	}
	return turn.FileChangeCount > 0
}

func hasShellEditEvidence(turn *rollout.Turn) bool {
	for _, call := range turn.FunctionCalls {
		if isShellEditCall(call) {
			return true
		}
	}
	return turn.CommandExecutionCount > 0
}

func isApplyPatchCall(call rollout.FunctionCall) bool {
	return strings.Contains(canonicalCallName(call), "apply_patch")
}

func isShellEditCall(call rollout.FunctionCall) bool {
	name := canonicalCallName(call)
	return strings.Contains(name, "exec_command") ||
		strings.Contains(name, "write_stdin") ||
		name == "shell" ||
		name == "command_execution"
}

func canonicalCallName(call rollout.FunctionCall) string {
	if call.Namespace != "" {
		return call.Namespace + "." + call.Name
	}
	return call.Name
}
