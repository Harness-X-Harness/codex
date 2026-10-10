package live_test

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

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

// The exact package and the native binary use the same public Live oracle.
// The HTTP fixture supplies the tool operation, so backend behavior is excluded.
func TestNativeStructuredEditFixture(t *testing.T) {
	binary := os.Getenv("GROK_LIVE_NATIVE_BIN")
	if binary == "" {
		t.Skip("activated by native runtime or explicit package diagnostic")
	}
	subject := nativeSubject(t, binary)
	if source := os.Getenv("GROK_LIVE_NATIVE_SOURCE_SHA"); source != "" {
		subject.SourceSHA = source
	}
	beforeHomes, err := filepath.Glob(filepath.Join(os.TempDir(), "grok-live-*"))
	if err != nil {
		t.Fatal(err)
	}
	existing := make(map[string]bool)
	for _, home := range beforeHomes {
		existing[home] = true
	}
	var requests atomic.Int32
	var invalid atomic.Bool
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
			body.Model != providerfixture.PrimaryModel || len(body.Tools) != 1 || !body.Stream || body.Store {
			invalid.Store(true)
		} else {
			var tool struct{ Type, Name string }
			if json.Unmarshal(body.Tools[0], &tool) != nil || tool.Type != "function" || tool.Name != "structured_edit" {
				invalid.Store(true)
			}
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
		outputs := 0
		for _, raw := range body.Input {
			var item map[string]json.RawMessage
			if json.Unmarshal(raw, &item) != nil {
				invalid.Store(true)
				continue
			}
			var kind, call string
			_ = json.Unmarshal(item["type"], &kind)
			_ = json.Unmarshal(item["call_id"], &call)
			if kind != "function_call_output" {
				continue
			}
			outputs++
			if call != "native_edit_call" {
				invalid.Store(true)
			}
			output := item["output"]
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
		if (number == 1 && outputs != 0) || (number > 1 && outputs != 1) || number > 3 {
			invalid.Store(true)
		}
		writer.Header().Set("Content-Type", "text/event-stream")
		emit := func(event any) {
			encoded, _ := json.Marshal(event)
			_, _ = fmt.Fprintf(writer, "data: %s\n\n", encoded)
		}
		responseID := fmt.Sprintf("native_edit_response_%d", number)
		emit(map[string]any{"type": "response.created", "response": map[string]any{"id": responseID}})
		if number == 1 {
			args, _ := json.Marshal(map[string]any{"file_path": "structured_edit_fixture.txt", "old_string": "GROK_STRUCTURED_EDIT_SEED_v1", "new_string": "GROK_STRUCTURED_EDIT_REPLACED_v1", "replace_all": false})
			added := map[string]any{"type": "function_call", "id": "native_edit_item", "call_id": "native_edit_call", "name": "structured_edit", "arguments": ""}
			emit(map[string]any{"type": "response.output_item.added", "output_index": 0, "item": added})
			emit(map[string]any{"type": "response.function_call_arguments.delta", "output_index": 0, "delta": string(args)})
			added["arguments"], added["status"] = string(args), "completed"
			emit(map[string]any{"type": "response.output_item.done", "output_index": 0, "item": added})
		} else {
			message := map[string]any{"type": "message", "id": fmt.Sprintf("native_edit_message_%d", number), "role": "assistant", "phase": "final_answer", "status": "completed", "content": []any{map[string]any{"type": "output_text", "text": "native edit complete"}}}
			emit(map[string]any{"type": "response.output_item.added", "output_index": 0, "item": map[string]any{"type": "message", "id": message["id"], "role": "assistant", "phase": "final_answer", "content": []any{}}})
			emit(map[string]any{"type": "response.output_text.delta", "output_index": 0, "delta": "native edit complete"})
			emit(map[string]any{"type": "response.output_item.done", "output_index": 0, "item": message})
		}
		emit(map[string]any{"type": "response.completed", "response": map[string]any{"id": responseID}})
	}))
	defer backend.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	got, err := live.StructuredEdit(ctx, live.Options{Subject: subject, Model: providerfixture.PrimaryModel, BaseURL: backend.URL + "/v1", APIKey: "fixture-key"})
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
	if !got.Completed || !got.TurnCompleted || !got.BytesMatch || !got.ContinuationUnchanged ||
		got.Calls != 1 || got.PairedOutputs != 1 || got.FileChanges != 1 || got.Approvals != 0 ||
		got.Turns != 2 || got.Processes != 1 || got.Stage != "edit_continuation_observed" ||
		requests.Load() != 3 || invalid.Load() {
		t.Fatalf("native structured-edit evidence mismatch: %+v", got)
	}
}
