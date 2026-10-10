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
	"regexp"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
	"unicode"
	"unicode/utf8"

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
			body.Model != providerfixture.PrimaryModel || !body.Stream || body.Store {
			invalid.Store(true)
			http.Error(writer, "invalid fixture request", http.StatusBadRequest)
			return
		}
		wireName, ok := nativeStructuredEditToolName(body.Tools)
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
		outputs := 0
		var pairedOutput json.RawMessage
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
				continue
			}
			output := item["output"]
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
		if (number == 1 && outputs != 0) || (number > 1 && outputs != 1) || number > 3 {
			invalid.Store(true)
		}
		if outputs > 0 {
			diagnosticMu.Lock()
			if outputs == 1 && !invalid.Load() {
				diagnostic["controlled_failure_detail"] = nativeEditFailureDetail(pairedOutput)
			} else {
				diagnostic["controlled_failure_detail"] = map[string]any{"captured": false, "format": "invalid_output_binding"}
			}
			diagnosticMu.Unlock()
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
			added := map[string]any{"type": "function_call", "id": "native_edit_item", "call_id": "native_edit_call", "name": wireName, "arguments": ""}
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

// This fixture is tied to the plain structured_edit capability in the identified
// Grok package. The name is flat_wire_name("function", ToolName::plain(...));
// return the declaration's name so the response uses the actual wire identity.
func nativeStructuredEditToolName(tools []json.RawMessage) (string, bool) {
	const projectedName = "local___structured_edit_418464e5de0f7b1e"
	var bound string
	hosted := false
	for _, raw := range tools {
		var tool struct {
			Type, Name string
			Parameters struct {
				Type                 string
				Properties           map[string]struct{ Type string }
				Required             []string
				AdditionalProperties *bool
			}
		}
		if json.Unmarshal(raw, &tool) != nil {
			return "", false
		}
		switch tool.Type {
		case "x_search":
			// This scenario configures no date options.
			var fields map[string]json.RawMessage
			if json.Unmarshal(raw, &fields) != nil || len(fields) != 1 || hosted {
				return "", false
			}
			hosted = true
		case "function":
			schema := tool.Parameters
			if bound != "" || tool.Name != projectedName || schema.Type != "object" ||
				schema.AdditionalProperties == nil || *schema.AdditionalProperties ||
				len(schema.Required) != 3 || len(schema.Properties) < 4 || len(schema.Properties) > 5 {
				return "", false
			}
			required := map[string]bool{"file_path": false, "old_string": false, "new_string": false}
			for _, name := range schema.Required {
				seen, known := required[name]
				if !known || seen {
					return "", false
				}
				required[name] = true
			}
			for name, property := range schema.Properties {
				expected := "string"
				switch name {
				case "file_path", "old_string", "new_string", "environment_id":
				case "replace_all":
					expected = "boolean"
				default:
					return "", false
				}
				if property.Type != expected {
					return "", false
				}
			}
			for _, name := range []string{"file_path", "old_string", "new_string", "replace_all"} {
				if _, exists := schema.Properties[name]; !exists {
					return "", false
				}
			}
			bound = tool.Name
		default:
			return "", false
		}
	}
	return bound, bound != ""
}

func TestNativeStructuredEditDeclaration(t *testing.T) {
	// Mirrors flat_projection + grok_request output, including the independently
	// appended hosted tool. Descriptions do not alter argument validation.
	const declaration = `{"type":"function","name":"local___structured_edit_418464e5de0f7b1e","description":"Edit an existing file","parameters":{"type":"object","properties":{"file_path":{"type":"string"},"old_string":{"type":"string"},"new_string":{"type":"string"},"replace_all":{"type":"boolean"}},"required":["file_path","old_string","new_string"],"additionalProperties":false}}`
	edit := json.RawMessage(declaration)
	hosted := json.RawMessage(`{"type":"x_search"}`)
	for _, tc := range []struct {
		name  string
		tools []json.RawMessage
		want  bool
	}{
		{"projected_with_hosted", []json.RawMessage{edit, hosted}, true},
		{"hosted_first", []json.RawMessage{hosted, edit}, true},
		{"projected_alone", []json.RawMessage{edit}, true},
		{"plain_name", []json.RawMessage{json.RawMessage(strings.Replace(declaration, "local___structured_edit_418464e5de0f7b1e", "structured_edit", 1))}, false},
		{"wrong_identity", []json.RawMessage{json.RawMessage(strings.Replace(declaration, "418464e5de0f7b1e", "418464e5de0f7b1f", 1))}, false},
		{"duplicate_edit", []json.RawMessage{edit, edit, hosted}, false},
		{"missing_edit", []json.RawMessage{hosted}, false},
		{"duplicate_hosted", []json.RawMessage{edit, hosted, hosted}, false},
		{"other_tool", []json.RawMessage{edit, json.RawMessage(`{"type":"web_search"}`)}, false},
		{"wrong_parameter_type", []json.RawMessage{json.RawMessage(strings.Replace(declaration, `"type":"boolean"`, `"type":"string"`, 1))}, false},
		{"missing_required", []json.RawMessage{json.RawMessage(strings.Replace(declaration, `["file_path","old_string","new_string"]`, `["file_path","old_string"]`, 1))}, false},
		{"duplicate_required", []json.RawMessage{json.RawMessage(strings.Replace(declaration, `["file_path","old_string","new_string"]`, `["file_path","old_string","old_string"]`, 1))}, false},
		{"additional_arguments", []json.RawMessage{json.RawMessage(strings.Replace(declaration, `"additionalProperties":false`, `"additionalProperties":true`, 1))}, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			name, ok := nativeStructuredEditToolName(tc.tools)
			if ok != tc.want || (ok && name != "local___structured_edit_418464e5de0f7b1e") {
				t.Fatalf("declaration binding = %q, %v; want admitted=%v", name, ok, tc.want)
			}
		})
	}
}

// Only used for the owned synthetic edit in the loopback fixture above. Real
// backend scenarios never retain tool-output text. Keep error operations and
// numeric errno/status while removing paths and credential/environment lines.
func nativeEditFailureDetail(raw json.RawMessage) map[string]any {
	var text string
	if !strings.HasPrefix(strings.TrimSpace(string(raw)), "\"") || json.Unmarshal(raw, &text) != nil {
		return map[string]any{"format": "unsupported", "captured": false}
	}
	const limit = 4096
	truncated := false
	// The HTTP body is already capped at 1 MiB. Sanitize before output truncation.
	ansi := regexp.MustCompile("\x1b\\[[0-9;]*[A-Za-z]")
	text = ansi.ReplaceAllString(text, "")
	text = strings.Map(func(r rune) rune {
		if unicode.IsControl(r) && r != '\n' && r != '\t' {
			return -1
		}
		return r
	}, text)
	credentials := regexp.MustCompile(`\b[A-Z][A-Z0-9_]*=|(?i:\b(?:authorization|bearer|api[_-]?key|access[_-]?token|token|password|secret)\b\s*[:= ]|gh[pousr]_[A-Za-z0-9]+|github_pat_[A-Za-z0-9_]+|sk-[A-Za-z0-9]+)`)
	paths := regexp.MustCompile(`/[^\s"'<>\\:]+`)
	lines := strings.Split(text, "\n")
	filtered := 0
	for i, line := range lines {
		if credentials.MatchString(line) {
			lines[i] = "[filtered credential/environment line]"
			filtered++
			continue
		}
		lines[i] = paths.ReplaceAllStringFunc(line, func(path string) string {
			for _, suffix := range []string{"/uid_map", "/gid_map", "/setgroups", "/codex-linux-sandbox", "/structured_edit_fixture.txt"} {
				if strings.HasSuffix(path, suffix) {
					return "<path:" + strings.TrimPrefix(suffix, "/") + ">"
				}
			}
			return "<path>"
		})
	}
	text = strings.Join(lines, "\n")
	if len(text) > limit {
		truncated = true
		text = text[:limit]
		for !utf8.ValidString(text) {
			text = text[:len(text)-1]
		}
	}
	return map[string]any{"format": "json_string", "captured": true, "text": text, "truncated": truncated, "filtered_lines": filtered}
}

func TestNativeEditFailureDetail(t *testing.T) {
	input := "Failed to write file /tmp/grok-live-owned/workspace/structured_edit_fixture.txt\n" +
		"fs sandbox helper failed with status exit status: 1: bwrap: setting up uid map: Permission denied (os error 13)\n" +
		"open /proc/1234/gid_map: Operation not permitted (os error 1)\n" +
		"GROK_\x1b[31mAPI_KEY=synthetic-do-not-publish\nAuthorization: Bearer fixture-secret\n" +
		"errno=13 exit_code=1 operation=write"
	raw, _ := json.Marshal(input)
	got := nativeEditFailureDetail(raw)
	text := got["text"].(string)
	for _, required := range []string{"<path:structured_edit_fixture.txt>", "<path:gid_map>", "setting up uid map", "exit status: 1", "os error 13", "errno=13 exit_code=1 operation=write"} {
		if !strings.Contains(text, required) {
			t.Fatalf("lost diagnostic field %q", required)
		}
	}
	for _, forbidden := range []string{"/tmp/", "/proc/", "synthetic-do-not-publish", "fixture-secret", "GROK_API_KEY", "Authorization"} {
		if strings.Contains(text, forbidden) {
			t.Fatalf("unfiltered value %q", forbidden)
		}
	}
	if got["filtered_lines"] != 2 || got["truncated"] != false || got["captured"] != true {
		t.Fatalf("unexpected detail flags: %+v", got)
	}

	long, _ := json.Marshal("bwrap: Creating new namespace failed: Operation not permitted\n" + strings.Repeat("é", 3000))
	bounded := nativeEditFailureDetail(long)
	if bounded["truncated"] != true || !strings.HasPrefix(bounded["text"].(string), "bwrap: Creating new namespace failed") || len(bounded["text"].(string)) > 4096 || !utf8.ValidString(bounded["text"].(string)) {
		t.Fatal("bounded diagnostic lost prefix, exceeded bound, or split UTF-8")
	}
	if nativeEditFailureDetail(json.RawMessage(`{"unexpected":"object"}`))["captured"] != false || nativeEditFailureDetail(json.RawMessage(`null`))["captured"] != false {
		t.Fatal("accepted unsupported output structure")
	}
}
