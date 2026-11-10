//go:build integration

package integration_test

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/integrationfixture"
	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

// The exact package and the native binary use the same public Live oracle.
// The HTTP fixture supplies the tool operation, so backend behavior is excluded.
func TestStructuredEditFixture(t *testing.T) {
	for _, mode := range []string{"continuation", "pinned", "declined", "replace_all"} {
		t.Run(mode, func(t *testing.T) { testNativeStructuredEditFixture(t, mode) })
	}
}

func testNativeStructuredEditFixture(t *testing.T, mode string) {
	integrationObservation(t)
	run, model := live.StructuredEdit, providerfixture.PrimaryModel
	wantTurns, wantApprovals := 2, 0
	wantContinuation, replaceAll, declined := true, true, false
	switch mode {
	case "pinned":
		run, model = live.StructuredEditPinnedPreviousModel, providerfixture.PinnedModel
		wantTurns, wantContinuation, replaceAll = 1, false, false
	case "declined":
		run = live.StructuredEditApprovalDeclined
		wantTurns, wantApprovals, wantContinuation, replaceAll, declined = 1, 1, false, false, true
	case "replace_all":
		run = live.StructuredEditReplaceAll
		wantTurns, wantContinuation, replaceAll = 1, false, true
	}
	seed := "Shop notice\r\n" + editSeedToken + "\r\nThank you."
	if replaceAll {
		seed += "\r\n" + editSeedToken
	}
	expected := strings.ReplaceAll(seed, editSeedToken, editReplacedToken)
	if declined {
		expected = seed
	}
	digest := func(text string) string {
		sum := sha256.Sum256([]byte(text))
		return hex.EncodeToString(sum[:])
	}
	arguments, _ := json.Marshal(map[string]any{"file_path": "store-hours.txt", "old_string": editSeedToken, "new_string": editReplacedToken, "replace_all": replaceAll})
	subject := nativeSubject(t)
	beforeHomes, err := filepath.Glob(filepath.Join(os.TempDir(), "grok-live-*"))
	if err != nil {
		t.Fatal(err)
	}
	existing := make(map[string]bool)
	for _, home := range beforeHomes {
		existing[home] = true
	}
	var requests, compositions atomic.Int32
	var invalid atomic.Bool
	var historyMu sync.Mutex
	var issued, paired bool
	var resultHistory integrationfixture.EditResultHistory
	var diagnosticMu sync.Mutex
	_, pathAliasErr := exec.LookPath("codex-linux-sandbox")
	diagnostic := map[string]any{"alias_state": "unobserved", "baseline_path_alias_available": pathAliasErr == nil, "tool_output_class": "unobserved"}
	backend := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		defer request.Body.Close()
		number := requests.Add(1)
		var body struct {
			Model         string
			Tools         []json.RawMessage
			Input         []json.RawMessage
			Stream, Store bool
		}
		if json.NewDecoder(io.LimitReader(request.Body, 1<<20)).Decode(&body) != nil ||
			request.Method != "POST" || request.URL.Path != "/v1/responses" ||
			request.Header.Get("Authorization") != "Bearer fixture-key" ||
			body.Model != model || !body.Stream || body.Store {
			invalid.Store(true)
			http.Error(writer, "invalid fixture request", http.StatusBadRequest)
			return
		}
		wireName, ok := integrationfixture.StructuredEditToolName(body.Tools)
		if !ok {
			invalid.Store(true)
			http.Error(writer, "invalid fixture declarations", http.StatusBadRequest)
			return
		}
		if number == 1 {
			homes, _ := filepath.Glob(filepath.Join(os.TempDir(), "grok-live-*"))
			var created []string
			for _, home := range homes {
				if !existing[home] {
					created = append(created, home)
				}
			}
			state := "ambiguous"
			if len(created) == 1 {
				aliases, _ := filepath.Glob(filepath.Join(created[0], "tmp", "arg0", "codex-arg0*", "codex-linux-sandbox"))
				state = "absent"
				if len(aliases) != 0 {
					state = "present"
				}
			}
			diagnosticMu.Lock()
			diagnostic["alias_state"] = state
			diagnosticMu.Unlock()
		}
		calls, outputs, userTurns, unknownItems := 0, 0, 0, 0
		var pairedOutput json.RawMessage
		for _, raw := range body.Input {
			var item map[string]json.RawMessage
			if json.Unmarshal(raw, &item) != nil {
				invalid.Store(true)
				continue
			}
			var kind, call string
			if value, present := item["type"]; present && json.Unmarshal(value, &kind) != nil {
				invalid.Store(true)
				continue
			}
			_ = json.Unmarshal(item["call_id"], &call)
			switch kind {
			case "", "message":
				var role string
				_ = json.Unmarshal(item["role"], &role)
				if role == "user" {
					userTurns++
				} else if role != "developer" && role != "system" && role != "assistant" {
					invalid.Store(true)
				}
			case "function_call":
				calls++
				var name, args string
				if json.Unmarshal(item["name"], &name) != nil || json.Unmarshal(item["arguments"], &args) != nil ||
					call != "native_edit_call" || name != wireName || !integrationfixture.SameEditArguments(args, string(arguments)) {
					invalid.Store(true)
				}
			case "function_call_output":
			case "reasoning":
			default:
				// No alternative tool history can explain this controlled edit.
				unknownItems++
				invalid.Store(true)
			}
			if kind != "function_call_output" {
				continue
			}
			outputs++
			if call != "native_edit_call" {
				invalid.Store(true)
				continue
			}
			output := item["output"]
			if !resultHistory.Accept(output) {
				invalid.Store(true)
				continue
			}
			pairedOutput = output
			sum := sha256.Sum256(output)
			lower := strings.ToLower(string(output))
			class := "other_output"
			switch {
			case strings.Contains(lower, "permission denied"), strings.Contains(lower, "operation not permitted"):
				class = "contains_permission_denied_text"
			case strings.Contains(lower, "no such file or directory"), strings.Contains(lower, "not found"):
				class = "contains_missing_path_text"
			}
			diagnosticMu.Lock()
			diagnostic["tool_output_class"] = class
			diagnostic["tool_output_bytes"] = len(output)
			diagnostic["tool_output_sha256"] = hex.EncodeToString(sum[:])
			diagnosticMu.Unlock()
		}
		diagnosticMu.Lock()
		diagnostic["history_calls"], diagnostic["history_outputs"], diagnostic["history_user_messages"], diagnostic["history_unknown_items"] = calls, outputs, userTurns, unknownItems
		diagnosticMu.Unlock()
		// Requests are driven by semantic history, not an exact transport count.
		// A retry of the initial request gets the same call identity; once the
		// paired result is present, no subsequent request may lose or repeat it.
		historyMu.Lock()
		if calls == 0 && outputs == 0 && !paired {
			issued = true
		} else if calls == 1 && outputs == 1 && issued {
			paired = true
		} else {
			invalid.Store(true)
		}
		historyMu.Unlock()
		if outputs > 0 {
			diagnosticMu.Lock()
			if outputs == 1 && !invalid.Load() {
				diagnostic["controlled_failure_detail"] = integrationfixture.EditFailureDetail(pairedOutput)
			} else {
				diagnostic["controlled_failure_detail"] = map[string]any{"captured": false, "format": "invalid_output_binding"}
			}
			diagnosticMu.Unlock()
		}
		if invalid.Load() {
			http.Error(writer, "invalid fixture history", http.StatusBadRequest)
			return
		}
		writer.Header().Set("Content-Type", "text/event-stream")
		writer.WriteHeader(http.StatusOK)
		emit := func(event any) {
			encoded, _ := json.Marshal(event)
			if _, err := fmt.Fprintf(writer, "data: %s\n\n", encoded); err != nil {
				invalid.Store(true)
			}
		}
		responseID := fmt.Sprintf("native_edit_response_%d", number)
		emit(map[string]any{"type": "response.created", "response": map[string]any{"id": responseID}})
		if calls == 0 {
			added := map[string]any{"type": "function_call", "id": "native_edit_item", "call_id": "native_edit_call", "name": wireName, "arguments": ""}
			emit(map[string]any{"type": "response.output_item.added", "output_index": 0, "item": added})
			emit(map[string]any{"type": "response.function_call_arguments.delta", "output_index": 0, "delta": string(arguments)})
			added["arguments"], added["status"] = string(arguments), "completed"
			emit(map[string]any{"type": "response.output_item.done", "output_index": 0, "item": added})
		} else {
			message := map[string]any{"type": "message", "id": fmt.Sprintf("native_edit_message_%d", number), "role": "assistant", "phase": "final_answer", "status": "completed", "content": []any{map[string]any{"type": "output_text", "text": "native edit complete"}}}
			emit(map[string]any{"type": "response.output_item.added", "output_index": 0, "item": map[string]any{"type": "message", "id": message["id"], "role": "assistant", "phase": "final_answer", "content": []any{}}})
			emit(map[string]any{"type": "response.output_text.delta", "output_index": 0, "delta": "native edit complete"})
			emit(map[string]any{"type": "response.output_item.done", "output_index": 0, "item": message})
		}
		emit(map[string]any{"type": "response.completed", "response": map[string]any{"id": responseID}})
		if !invalid.Load() {
			compositions.Add(1)
		}
	}))
	defer backend.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	got, err := run(ctx, live.Options{Subject: subject, Model: model, BaseURL: backend.URL + "/v1", APIKey: "fixture-key"})
	diagnosticMu.Lock()
	diagnostic["source_sha"], diagnostic["harness_sha"], diagnostic["target"] = subject.SourceSHA, subject.HarnessSHA, subject.Target
	diagnostic["executable_sha256"], diagnostic["requests"], diagnostic["invalid_request"] = subject.SHA256, requests.Load(), invalid.Load()
	diagnostic["outcome"] = "observed"
	if err != nil {
		diagnostic["outcome"] = "not_observed"
		diagnostic["failure"] = live.DescribeFailure(err)
	}
	encoded, _ := json.Marshal(diagnostic)
	diagnosticMu.Unlock()
	t.Logf("controlled structured-edit diagnostics: %s", encoded)
	if err != nil {
		t.Fatalf("native structured-edit oracle: %v", err)
	}
	// Check the scenario's facts directly. Additional evidence fields and reply
	// wording are not an acceptance contract; identity, effects and permissions are.
	if got.CleanupPending || got.LocalUnverified != subject.LocalUnverified || got.SHA256 != subject.SHA256 ||
		got.SourceSHA != subject.SourceSHA || got.HarnessSHA != subject.HarnessSHA ||
		got.Target != subject.Target || got.Environment != subject.Environment || got.Model != model ||
		got.Processes != 1 || got.Initializations != 1 || got.Threads != 1 || got.Turns != wantTurns ||
		got.ReplyBytes <= 0 || got.FirstReplyBytes <= 0 || !got.Bound || !got.Completed || !got.FirstCompleted || !got.TurnCompleted ||
		got.Approvals != wantApprovals ||
		got.BeforeSHA256 != digest(seed) || got.AfterSHA256 != digest(expected) || !got.BytesMatch || got.ContinuationUnchanged != wantContinuation {
		t.Fatalf("native structured-edit evidence mismatch: %+v", got)
	}
	historyMu.Lock()
	observedPair := issued && paired
	historyMu.Unlock()
	if !observedPair || compositions.Load() == 0 || invalid.Load() {
		t.Fatal("native structured-edit call/result history was not preserved")
	}
}

const editSeedToken = "The shop opens at 8 AM."
const editReplacedToken = "The shop opens at 9 AM."
