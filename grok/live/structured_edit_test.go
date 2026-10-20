package live_test

import (
	"bufio"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

const editSeedToken = "GROK_STRUCTURED_EDIT_SEED_v1"
const editReplacedToken = "GROK_STRUCTURED_EDIT_REPLACED_v1"

// This is a protocol/runner oracle. It deliberately performs the modeled file
// effects, but is neither the native product editor nor a Grok backend.
func fakeEditServer() {
	var script struct{ Mode, Trace string }
	if json.Unmarshal([]byte(os.Getenv("GROK_API_KEY")), &script) != nil {
		return
	}
	mode := strings.TrimPrefix(script.Mode, "edit:")
	early := mode == "early" || mode == "early_decline"
	trace, err := os.OpenFile(script.Trace, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
	if err != nil {
		return
	}
	defer trace.Close()
	// SDK cleanup interrupts the child before closing stdin. Keep that signal
	// from skipping the intended phase, including a joined-shutdown mutation.
	signal.Notify(make(chan os.Signal, 1), os.Interrupt)
	log := func(value string) { _, _ = io.WriteString(trace, value+"\n") }
	log("process")
	input, output := bufio.NewScanner(os.Stdin), json.NewEncoder(os.Stdout)
	send := func(method string, params any) { _ = output.Encode(map[string]any{"method": method, "params": params}) }
	path, _ := filepath.Abs("structured_edit_fixture.txt")
	seed, err := os.ReadFile(path)
	if err != nil {
		return
	}
	defer func() {
		log("eof")
		if mode == "shutdown_changes" {
			_ = os.WriteFile(path, []byte("PRIVATE_CANARY"), 0600)
			log("file_written")
		}
	}()
	replaceAll := strings.Count(string(seed), editSeedToken) == 2
	var turns []any
	var boundThread map[string]any
	decline := false
	for input.Scan() {
		var request struct {
			ID     json.RawMessage
			Method string
			Params json.RawMessage
		}
		if json.Unmarshal(input.Bytes(), &request) != nil {
			return
		}
		log(request.Method)
		var result any
		switch request.Method {
		case "initialize":
			family := "unix"
			if runtime.GOOS == "windows" {
				family = "windows"
			}
			result = map[string]any{"userAgent": "edit-fixture", "codexHome": os.Getenv("CODEX_HOME"), "platformFamily": family, "platformOs": runtime.GOOS}
		case "initialized":
			continue
		case "thread/start":
			var params struct {
				Model, Cwd, ApprovalPolicy string
				ExperimentalRawEvents      bool
			}
			if json.Unmarshal(request.Params, &params) != nil || params.Cwd != filepath.Dir(path) || !params.ExperimentalRawEvents {
				return
			}
			decline = params.ApprovalPolicy == "untrusted"
			if !decline && params.ApprovalPolicy != "never" {
				return
			}
			var catalog struct {
				Models []struct {
					Slug       string `json:"slug"`
					Structured string `json:"structured_edit_tool_type"`
					Patch      any    `json:"apply_patch_tool_type"`
					Shell      string `json:"shell_type"`
				}
			}
			data, err := os.ReadFile(filepath.Join(os.Getenv("CODEX_HOME"), "models.json"))
			if err != nil || json.Unmarshal(data, &catalog) != nil || len(catalog.Models) != 1 ||
				catalog.Models[0].Slug != params.Model || catalog.Models[0].Structured != "exact_match" ||
				catalog.Models[0].Patch != nil || catalog.Models[0].Shell != "disabled" {
				return
			}
			config, err := os.ReadFile(filepath.Join(os.Getenv("CODEX_HOME"), "config.toml"))
			if err != nil || !strings.Contains(string(config), `sandbox_mode = "workspace-write"`) {
				return
			}
			log("capability_fixture")
			model := params.Model
			if mode == "wrong_model" {
				model = "PRIVATE_CANARY"
			}
			boundThread = map[string]any{
				"id": "private-thread", "sessionId": "private-session", "modelProvider": "grok", "cliVersion": "edit-fixture",
				"createdAt": 1, "updatedAt": 1, "cwd": params.Cwd, "ephemeral": false, "preview": "", "projectId": nil,
				"source": "appServer", "status": map[string]any{"type": "idle"}, "turns": []any{},
			}
			result = map[string]any{
				"model": model, "modelProvider": "grok", "cwd": params.Cwd, "approvalPolicy": params.ApprovalPolicy, "approvalsReviewer": "user",
				"sandbox": map[string]any{"type": "workspaceWrite", "writableRoots": []any{params.Cwd}, "networkAccess": false, "excludeTmpdirEnvVar": false, "excludeSlashTmp": false},
				"thread":  boundThread,
			}
		case "turn/start":
			if mode == "stalled_start" {
				for input.Scan() {
				}
				return
			}
			var params struct {
				ThreadID string `json:"threadId"`
			}
			if json.Unmarshal(request.Params, &params) != nil || params.ThreadID != "private-thread" {
				return
			}
			first := len(turns) == 0
			turnID := "private-first"
			if !first {
				turnID = "private-second"
			}
			callID := "private-call"
			name := "structured_edit"
			if mode == "wire_identity" {
				name = "local__structured_edit__deadbeefcafe"
			}
			args := map[string]any{"file_path": "structured_edit_fixture.txt", "old_string": editSeedToken, "new_string": editReplacedToken}
			if replaceAll {
				args["replace_all"] = true
			}
			if mode == "absolute_path" {
				args["file_path"] = path
			}
			if mode == "false_replace_all" {
				args["replace_all"] = false
			}
			switch mode {
			case "string_replace_all":
				args["replace_all"] = "true"
			case "null_replace_all":
				args["replace_all"] = nil
			case "number_replace_all":
				args["replace_all"] = 1
			case "missing_replace_all":
				delete(args, "replace_all")
			case "wrong_old":
				args["old_string"] = "PRIVATE_CANARY"
			case "extra_arg":
				args["extra"] = true
			case "wrong_path":
				args["file_path"] = "other.txt"
			case "near_identity":
				name = "not_structured_edit"
			case "near_wire_identity":
				name = "local__structured_edit__deadbeefcafeX"
			case "apply_patch":
				name = "apply_patch"
			}
			encoded, _ := json.Marshal(args)
			arguments := string(encoded)
			if mode == "malformed_args" {
				arguments = "PRIVATE_CANARY"
			}
			if mode == "duplicate_arg" {
				arguments = strings.TrimSuffix(arguments, "}") + `,"replace_all":true,"replace_all":false}`
			}
			call := map[string]any{"type": "function_call", "call_id": callID, "name": name, "arguments": arguments}
			if mode == "namespace" {
				call["namespace"] = "PRIVATE_CANARY"
			}
			toolOutput := map[string]any{"type": "function_call_output", "call_id": callID, "output": "fixture output PRIVATE_CANARY"}
			if mode == "wrong_output" {
				toolOutput["call_id"] = "PRIVATE_CANARY"
			}
			if mode == "empty_output" {
				toolOutput["output"] = ""
			}
			if mode == "malformed_output" {
				toolOutput["output"] = map[string]any{"success": true}
			}
			if mode == "content_output" {
				toolOutput["output"] = []any{map[string]any{"type": "input_text", "text": "fixture output"}}
			}
			status := "completed"
			if decline {
				status = "declined"
			}
			switch mode {
			case "failed_change":
				status = "failed"
			case "declined_change":
				status = "declined"
			case "running_change":
				status = "inProgress"
			case "unknown_change":
				status = "PRIVATE_CANARY"
			}
			changePath := path
			if mode == "wrong_change_path" {
				changePath = filepath.Join(filepath.Dir(path), "other.txt")
			}
			change := map[string]any{"type": "fileChange", "id": callID, "status": status, "changes": []any{
				map[string]any{"path": changePath, "kind": map[string]any{"type": "update", "move_path": nil}, "diff": "fixture diff"},
			}}
			if mode == "wrong_change_id" {
				change["id"] = "PRIVATE_CANARY"
			}
			if mode == "move_change" {
				change["changes"].([]any)[0].(map[string]any)["kind"] = map[string]any{"type": "update", "move_path": "PRIVATE_CANARY"}
			}
			agent := map[string]any{"type": "agentMessage", "id": "private-reply-" + turnID, "phase": "final_answer", "text": "Fixture complete."}
			if mode == "empty_reply" {
				agent["text"] = " "
			}
			if mode == "commentary" {
				agent["phase"] = "commentary"
			}
			items := []any{agent}
			if first && mode != "missing_change" {
				items = append([]any{change}, items...)
			}
			terminal := map[string]any{"id": turnID, "status": "completed", "items": items, "startedAt": 1, "completedAt": 1, "durationMs": 1, "error": nil}
			if mode == "failed" || mode == "interrupted" || mode == "continuation_failed" && !first {
				terminal["status"] = "failed"
				if mode == "interrupted" {
					terminal["status"] = "interrupted"
				}
				terminal["error"] = map[string]any{"message": "PRIVATE_CANARY"}
			}
			threadID, eventTurnID := "private-thread", turnID
			if mode == "wrong_thread" {
				threadID = "PRIVATE_CANARY"
			}
			if mode == "wrong_turn" {
				eventTurnID = "PRIVATE_CANARY"
			}
			raw := func(item any) {
				send("rawResponseItem/completed", map[string]any{"threadId": threadID, "turnId": eventTurnID, "item": item})
			}
			public := func(item any) {
				send("item/completed", map[string]any{"threadId": threadID, "turnId": eventTurnID, "item": item, "completedAtMs": 1})
			}
			if !early {
				_ = output.Encode(map[string]any{"id": request.ID, "result": map[string]any{"turn": map[string]any{"id": turnID, "status": "inProgress", "items": []any{}, "startedAt": 1}}})
			}
			if first {
				if mode == "output_first" {
					raw(toolOutput)
				}
				if mode != "missing_call" {
					raw(call)
				}
				if mode == "duplicate_call" {
					raw(call)
				}
				if decline || mode == "denied_effect" {
					approval := map[string]any{"threadId": "private-thread", "turnId": turnID, "itemId": callID, "startedAtMs": 1}
					if mode == "approval_thread" {
						approval["threadId"] = "PRIVATE_CANARY"
					}
					if mode == "approval_turn" {
						approval["turnId"] = "PRIVATE_CANARY"
					}
					if mode == "approval_item" {
						approval["itemId"] = "PRIVATE_CANARY"
					}
					approve := func(id int, candidate map[string]any) bool {
						_ = output.Encode(map[string]any{"id": id, "method": "item/fileChange/requestApproval", "params": candidate})
						var response struct {
							ID     int
							Result struct{ Decision string }
							Error  any
						}
						if !input.Scan() || json.Unmarshal(input.Bytes(), &response) != nil || response.ID != id || response.Result.Decision != "decline" || response.Error != nil {
							return false
						}
						log("approval_declined")
						return true
					}
					if !approve(99, approval) {
						return
					}
					if mode == "duplicate_approval" && !approve(100, approval) {
						return
					}
				}
				if (!decline || mode == "decline_changes") && mode != "false_success" {
					count := 1
					if replaceAll {
						count = -1
					}
					replacement := strings.Replace(string(seed), editSeedToken, editReplacedToken, count)
					if mode == "wrong_bytes" {
						replacement += "\n"
					}
					if os.WriteFile(path, []byte(replacement), 0600) != nil {
						return
					}
					log("file_written")
				}
				if mode != "missing_output" && mode != "output_first" {
					raw(toolOutput)
				}
				if mode == "duplicate_output" {
					raw(toolOutput)
				}
				if mode != "missing_change" {
					public(change)
				}
				if mode == "duplicate_edit" {
					if os.WriteFile(path, []byte(strings.ReplaceAll(string(seed), editSeedToken, editReplacedToken)), 0600) != nil {
						return
					}
					log("file_written")
					call["call_id"] = "private-other-call"
					raw(call)
				}
				if mode == "duplicate_change" {
					change["id"] = "private-other-call"
					public(change)
				}
				if mode == "conflicting_change" {
					change["status"] = "declined"
					public(change)
				}
				if mode == "command" {
					public(map[string]any{
						"type": "commandExecution", "id": "private-command", "command": "PRIVATE_CANARY", "cwd": filepath.Dir(path),
						"commandActions": []any{}, "status": "completed",
					})
				}
			} else {
				if mode == "repeat_continuation" {
					raw(call)
				}
				if mode == "continuation_changes" || mode == "repeat_continuation" {
					if os.WriteFile(path, []byte("PRIVATE_CANARY"), 0600) != nil {
						return
					}
					log("file_written")
				}
				if mode == "stale_continuation" {
					eventTurnID = "private-first"
					raw(call)
				}
			}
			if early {
				_ = output.Encode(map[string]any{"id": request.ID, "result": map[string]any{"turn": map[string]any{"id": turnID, "status": "inProgress", "items": []any{}, "startedAt": 1}}})
			}
			turns = append(turns, terminal)
			if mode != "missing_terminal" {
				send("turn/completed", map[string]any{"threadId": threadID, "turn": terminal})
			}
			continue
		case "thread/read":
			if mode == "read_error" || mode == "continuation_read_error" && len(turns) == 2 {
				_ = output.Encode(map[string]any{"id": request.ID, "error": map[string]any{"code": -32603, "message": "PRIVATE_CANARY"}})
				return
			}
			if mode == "late_duplicate" {
				arguments, _ := json.Marshal(map[string]any{"file_path": "structured_edit_fixture.txt", "old_string": "PRIVATE_CANARY", "new_string": editReplacedToken})
				send("rawResponseItem/completed", map[string]any{"threadId": "private-thread", "turnId": "private-first", "item": map[string]any{"type": "function_call", "call_id": "private-call", "name": "structured_edit", "arguments": string(arguments)}})
			}
			threadID := "private-thread"
			if mode == "read_thread" {
				threadID = "PRIVATE_CANARY"
			}
			if mode == "read_failed" {
				turns[len(turns)-1].(map[string]any)["status"] = "failed"
			}
			thread := make(map[string]any, len(boundThread))
			for key, value := range boundThread {
				thread[key] = value
			}
			thread["id"], thread["turns"] = threadID, turns
			result = map[string]any{"thread": thread}
		default:
			return
		}
		_ = output.Encode(map[string]any{"id": request.ID, "result": result})
	}
}

func TestStructuredEditScenarios(t *testing.T) {
	for _, mode := range []string{"success", "early", "early_decline", "wire_identity", "absolute_path", "false_replace_all", "output_first", "content_output", "decline", "pinned", "replace_all", "duplicate_call", "duplicate_output", "stale_continuation", "duplicate_approval"} {
		t.Run(mode, func(t *testing.T) {
			options := fixtureOptions(t, "edit:"+mode)
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			runner := live.StructuredEdit
			wantTurns, wantApprovals, wantWrites := 2, 0, 1
			seed := "prefix\r\n" + editSeedToken + "\r\nsuffix"
			switch mode {
			case "decline", "early_decline", "duplicate_approval":
				runner, wantTurns, wantApprovals, wantWrites = live.StructuredEditApprovalDeclined, 1, 1, 0
			case "pinned":
				runner, wantTurns = live.StructuredEditPinnedPreviousModel, 1
			case "replace_all":
				runner, wantTurns = live.StructuredEditReplaceAll, 1
				seed += "\r\n" + editSeedToken
			}
			got, err := runner(ctx, options)
			if err != nil {
				t.Fatal(err)
			}
			want := strings.ReplaceAll(seed, editSeedToken, editReplacedToken)
			if wantApprovals == 1 {
				want = seed
			}
			hash := func(text string) string { sum := sha256.Sum256([]byte(text)); return hex.EncodeToString(sum[:]) }
			if got.Processes != 1 || got.Initializations != 1 || got.Threads != 1 || got.Turns != wantTurns ||
				!got.Bound || !got.FirstCompleted || !got.TurnCompleted || !got.Completed || !got.BytesMatch || got.ContinuationUnchanged != (wantTurns == 2) ||
				got.Calls != 1 || got.PairedOutputs != 1 || got.FileChanges != 1 || got.Approvals != wantApprovals ||
				got.BeforeSHA256 != hash(seed) || got.AfterSHA256 != hash(want) {
				t.Fatalf("unexpected safe evidence: %+v", got)
			}
			wantModel := providerfixture.PrimaryModel
			if mode == "pinned" {
				wantModel = providerfixture.PinnedModel
			}
			if got.Model != wantModel {
				t.Fatal("model pin was not consumed")
			}
			trace := editTrace(t, options)
			traceApprovals := wantApprovals
			if mode == "duplicate_approval" {
				traceApprovals = 2
			}
			for operation, count := range map[string]int{"process": 1, "initialize": 1, "initialized": 1, "thread/start": 1, "capability_fixture": 1, "turn/start": wantTurns, "thread/read": wantTurns, "file_written": wantWrites, "approval_declined": traceApprovals, "eof": 1} {
				if strings.Count(trace, operation+"\n") != count {
					t.Fatalf("unexpected %s count in %q", operation, trace)
				}
			}
			encoded, _ := json.Marshal(got)
			if strings.Contains(string(encoded), "private-") || strings.Contains(string(encoded), "PRIVATE_CANARY") || strings.Contains(string(encoded), editSeedToken) {
				t.Fatal("private evidence escaped")
			}
		})
	}
}

func TestStructuredEditRejectsFalseEvidence(t *testing.T) {
	modes := []string{"wrong_model", "wrong_thread", "wrong_turn", "near_identity", "near_wire_identity", "namespace", "apply_patch", "command",
		"missing_call", "missing_output", "wrong_output", "empty_output", "malformed_output", "duplicate_edit", "late_duplicate",
		"missing_change", "wrong_change_id", "wrong_change_path", "move_change", "failed_change", "declined_change", "running_change", "unknown_change", "duplicate_change", "conflicting_change", "false_success", "wrong_bytes",
		"malformed_args", "duplicate_arg", "wrong_old", "wrong_path", "extra_arg", "string_replace_all", "null_replace_all", "number_replace_all", "missing_replace_all",
		"failed", "interrupted", "empty_reply", "commentary", "read_thread", "read_failed", "read_error", "continuation_read_error", "continuation_failed", "repeat_continuation", "continuation_changes",
		"approval_thread", "approval_turn", "approval_item", "decline_changes", "denied_effect", "shutdown_changes", "stalled_start", "missing_terminal"}
	for _, mode := range modes {
		t.Run(mode, func(t *testing.T) {
			options := fixtureOptions(t, "edit:"+mode)
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
			defer cancel()
			runner := live.StructuredEdit
			if strings.HasPrefix(mode, "approval_") || mode == "decline_changes" {
				runner = live.StructuredEditApprovalDeclined
			}
			if strings.Contains(mode, "replace_all") {
				runner = live.StructuredEditReplaceAll
			}
			got, err := runner(ctx, options)
			if err == nil {
				t.Fatal("false edit evidence accepted")
			}
			if got.Completed || got.BytesMatch || got.ContinuationUnchanged {
				t.Fatalf("failure retained success evidence: %+v", got)
			}
			switch mode {
			case "false_success", "wrong_bytes", "read_failed", "read_error", "read_thread", "continuation_changes", "continuation_read_error", "continuation_failed":
				wantStage, wantReply, wantCurrent := "edit_turn_completed", len("Fixture complete."), true
				if strings.HasPrefix(mode, "continuation_") {
					wantStage = "edit_continuation_completed"
				}
				if mode == "continuation_failed" {
					wantStage, wantReply, wantCurrent = "edit_continuation_submitted", 0, false
				}
				wantBytes := ""
				switch mode {
				case "false_success":
					wantBytes = "prefix\r\n" + editSeedToken + "\r\nsuffix"
				case "wrong_bytes":
					wantBytes = "prefix\r\n" + editReplacedToken + "\r\nsuffix\n"
				case "continuation_changes":
					wantBytes = "PRIVATE_CANARY"
				}
				wantHash := ""
				if wantBytes != "" {
					sum := sha256.Sum256([]byte(wantBytes))
					wantHash = hex.EncodeToString(sum[:])
				}
				if !got.FirstCompleted || got.TurnCompleted != wantCurrent || got.Stage != wantStage || got.ReplyBytes != wantReply ||
					got.FirstReplyBytes != len("Fixture complete.") || got.AfterSHA256 != wantHash {
					t.Fatalf("incorrect partial evidence: %+v", got)
				}
			}
			wantError := "live: unexpected or invalid edit invocation"
			switch mode {
			case "wrong_model":
				wantError = "live: fixture binding not established"
			case "wrong_thread":
				wantError = "live: unrelated edit thread evidence"
			case "wrong_turn":
				wantError = "live: unrelated edit turn evidence"
			case "command":
				wantError = "live: alternative tool evidence observed"
			case "empty_output":
				wantError = "live: unexpected or invalid edit output"
			case "missing_call", "missing_output", "wrong_output", "missing_change", "wrong_change_id", "failed_change", "declined_change", "running_change":
				wantError = "live: edit call, output or file-change evidence missing"
			case "wrong_change_path", "move_change":
				wantError = "live: unexpected edit file change"
			case "duplicate_change":
				wantError = "live: conflicting or repeated edit file change"
			case "conflicting_change", "late_duplicate":
				wantError = "live: conflicting edit item evidence"
			case "false_success", "wrong_bytes", "continuation_changes", "decline_changes":
				wantError = "live: edit fixture bytes did not match"
			case "failed", "interrupted", "read_failed", "continuation_failed":
				wantError = "live: edit turn did not complete successfully"
			case "read_error", "continuation_read_error", "malformed_output", "unknown_change":
				wantError = "live: SDK observation did not complete"
			case "empty_reply", "commentary":
				wantError = "live: edit final reply unavailable"
			case "read_thread":
				wantError = "live: settled edit thread identity not established"
			case "approval_thread", "approval_turn", "approval_item":
				wantError = "live: edit call, output or file-change evidence missing"
			case "denied_effect":
				wantError = "live: unexpected edit approval denied"
			case "shutdown_changes":
				wantError = "live: edit fixture bytes did not match"
			case "stalled_start", "missing_terminal":
				wantError = "live: SDK observation did not complete"
			}
			// A schema failure closes the client. If a prior callback already stored
			// its context, the waiter sees sdk_observation_failed; otherwise it
			// reaches the deadline. Neither code is a completed edit.
			if mode == "malformed_output" || mode == "unknown_change" {
				code := live.DescribeFailure(err).Code
				if code != "deadline_exceeded" && code != "sdk_observation_failed" {
					t.Fatalf("schema failure proved an edit or lost its boundary: %+v", live.DescribeFailure(err))
				}
			} else if code, check := map[string]string{
				"missing_call": "edit_evidence_unpaired", "missing_output": "edit_evidence_unpaired",
				"wrong_output": "edit_evidence_unpaired", "missing_change": "edit_evidence_unpaired",
				"wrong_change_id": "edit_evidence_unpaired", "failed_change": "edit_evidence_unpaired",
				"declined_change": "edit_evidence_unpaired", "running_change": "edit_evidence_unpaired",
				"approval_item": "edit_evidence_unpaired", "approval_thread": "edit_evidence_unpaired",
				"approval_turn": "edit_evidence_unpaired", "denied_effect": "unexpected_edit_approval",
				"shutdown_changes": "edit_bytes_mismatch", "stalled_start": "deadline_exceeded",
				"missing_terminal": "deadline_exceeded", "read_error": "sdk_observation_failed",
				"continuation_read_error": "sdk_observation_failed",
				"failed":                  "turn_unsuccessful",
				"interrupted":             "turn_unsuccessful", "read_failed": "turn_unsuccessful",
				"continuation_failed": "turn_unsuccessful",
			}[mode]; check {
				if live.DescribeFailure(err).Code != code {
					t.Fatalf("incorrect bounded edit diagnostic: %+v, want %s", live.DescribeFailure(err), code)
				}
			}
			if err.Error() != wantError {
				t.Fatalf("oracle failed at the wrong boundary: %v, want %s", err, wantError)
			}
			encoded, _ := json.Marshal(got)
			if strings.Contains(string(encoded)+err.Error(), "PRIVATE_CANARY") || strings.Contains(string(encoded)+err.Error(), "private-") || strings.Contains(string(encoded)+err.Error(), options.Subject.Binary) {
				t.Fatal("private failure evidence escaped")
			}
			trace := editTrace(t, options)
			wantTurns := 1
			if mode == "wrong_model" {
				wantTurns = 0
			}
			if strings.Contains(mode, "continuation") || mode == "shutdown_changes" {
				wantTurns = 2
			}
			if mode == "missing_terminal" && strings.Contains(trace, "thread/read\n") {
				t.Fatal("settled Read synthesized an absent terminal")
			}
			if mode == "denied_effect" && (!strings.Contains(trace, "approval_declined\n") || !strings.Contains(trace, "file_written\n") || got.Completed || got.BytesMatch) {
				t.Fatal("matching denial was lost before the unauthorized effect")
			}
			if mode == "shutdown_changes" && (!got.FirstCompleted || !got.TurnCompleted || !strings.Contains(trace, "eof\n") || strings.Count(trace, "file_written\n") != 2) {
				t.Fatalf("joined shutdown did not reach its mutation phase: %+v %q", got, trace)
			}
			if strings.Count(trace, "process\n") != 1 || strings.Count(trace, "thread/start\n") != 1 || strings.Count(trace, "turn/start\n") != wantTurns {
				t.Fatal("unexpected semantic invocation count")
			}
			if mode == "false_success" && strings.Contains(trace, "file_written\n") {
				t.Fatal("false success fixture unexpectedly wrote bytes")
			}
			if mode == "decline_changes" && (!strings.Contains(trace, "approval_declined\n") || !strings.Contains(trace, "file_written\n")) {
				t.Fatal("decline mutation was not actually modeled")
			}
		})
	}
}

func editTrace(t *testing.T, options live.Options) string {
	t.Helper()
	var script struct{ Trace string }
	_ = json.Unmarshal([]byte(options.APIKey), &script)
	trace, err := os.ReadFile(script.Trace)
	if err != nil {
		t.Fatal(err)
	}
	return string(trace)
}
