//go:build integration

package integration_test

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/Harness-X-Harness/codex/grok/internal/integrationfixture"
)

// This drives only the controlled backend. It checks the oracle's value/identity
// boundary, not real provider inference or native product composition.
func testReasoningFixtureReplaySemantics(t *testing.T) {
	for _, tc := range []struct{ name, want string }{
		{"equivalent_json_string", ""},
		{"summary_whitespace", ""},
		{"summary_omitted", "encrypted_history_changed"},
		{"summary_null", "encrypted_history_changed"},
		{"summary_nonempty", "encrypted_history_changed"},
		{"changed_token", "tool_result_replay_changed"},
		{"changed_cipher", "encrypted_history_changed"},
		{"wrong_call", "tool_result_binding"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			integrationObservation(t)
			fixture := &reasoningHTTP{model: "fixture-model"}
			message := func(role, text string) map[string]any {
				return map[string]any{"type": "message", "role": role, "content": []any{map[string]any{"type": "input_text", "text": text}}}
			}
			request := func(input []any) *httptest.ResponseRecorder {
				t.Helper()
				body, err := json.Marshal(map[string]any{"model": fixture.model, "stream": true, "store": false,
					"include": []string{"reasoning.encrypted_content"}, "input": input,
					"tools": []any{map[string]any{"type": "function", "name": "emitted-probe-name", "description": "Call this function directly to invoke `grok_history_probe`. Do not invoke it through another tool.\n\n" + integrationfixture.HistoryProbeDescription}}})
				if err != nil {
					t.Fatal(err)
				}
				if tc.name == "summary_whitespace" {
					body = bytes.ReplaceAll(body, []byte(`"summary":[]`), []byte(`"summary": [ ]`))
				}
				r := httptest.NewRequest(http.MethodPost, "/v1/responses", bytes.NewReader(body))
				r.Header.Set("Authorization", "Bearer fixture-key")
				r.Header.Set("thread-id", "history-thread")
				w := httptest.NewRecorder()
				fixture.ServeHTTP(w, r)
				return w
			}
			input := []any{message("user", "Call grok_history_probe, then return its token.")}
			if request(input).Code != http.StatusOK {
				t.Fatal("fixture did not issue probe")
			}
			token := strings.Repeat("a", 64)
			reasoning := map[string]any{"type": "reasoning", "encrypted_content": reasoningHTTPCipher, "summary": []any{}}
			output := map[string]any{"type": "function_call_output", "call_id": reasoningHTTPCall, "output": token}
			input = append(input, reasoning,
				map[string]any{"type": "function_call", "call_id": reasoningHTTPCall, "name": "emitted-probe-name", "arguments": "{}"}, output)
			if request(input).Code != http.StatusOK {
				t.Fatal("fixture did not accept paired tool result")
			}
			input = append(input, message("assistant", token), message("user", "Recall the exact token returned by the tool in the previous turn without another tool."))
			switch tc.name {
			case "equivalent_json_string":
				output["output"] = json.RawMessage(`"\u0061` + strings.Repeat("a", 63) + `"`)
			case "summary_omitted":
				delete(reasoning, "summary")
			case "summary_null":
				reasoning["summary"] = nil
			case "summary_nonempty":
				reasoning["summary"] = []any{map[string]any{"type": "summary_text", "text": "unexpected summary"}}
			case "changed_token":
				output["output"] = strings.Repeat("b", 64)
			case "changed_cipher":
				reasoning["encrypted_content"] = "different-opaque-cipher"
			case "wrong_call":
				output["call_id"] = "another-call"
			}
			response := request(input)
			if fixture.failure != tc.want || (response.Code == http.StatusOK) != (tc.want == "") || fixture.continued != (tc.want == "") {
				t.Fatalf("replay boundary: status=%d failure=%s continued=%t", response.Code, fixture.failure, fixture.continued)
			}
		})
	}
}
