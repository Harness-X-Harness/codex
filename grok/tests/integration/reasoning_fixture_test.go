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
		{"changed_price", "tool_result_replay_changed"},
		{"changed_cipher", "encrypted_history_changed"},
		{"wrong_call", "tool_result_binding"},
		{"schema_omitted", "output_schema_changed"},
		{"schema_extra", "output_schema_changed"},
		{"schema_expected_amount", "output_schema_changed"},
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
					"text":  map[string]any{"format": map[string]any{"type": "json_schema", "name": "history", "strict": true, "schema": json.RawMessage(reasoningAmountSchema)}},
					"tools": []any{map[string]any{"type": "function", "name": "emitted-probe-name", "description": "Call this function directly to invoke `apple_price_lookup`. Do not invoke it through another tool.\n\n" + integrationfixture.HistoryProbeDescription}}})
				if err != nil {
					t.Fatal(err)
				}
				if fixture.requests == 2 && strings.HasPrefix(tc.name, "schema_") {
					var changed map[string]any
					_ = json.Unmarshal(body, &changed)
					if tc.name == "schema_omitted" {
						delete(changed, "text")
					} else {
						schema := changed["text"].(map[string]any)["format"].(map[string]any)["schema"].(map[string]any)
						if tc.name == "schema_extra" {
							schema["additionalProperties"] = true
						} else {
							schema["properties"].(map[string]any)["amount"].(map[string]any)["const"] = 14
						}
					}
					body, _ = json.Marshal(changed)
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
			input := []any{message("user", reasoningFirstPrompt)}
			if request(input).Code != http.StatusOK {
				t.Fatal("fixture did not issue probe")
			}
			priceResult := `{"unit_price_yuan":7}`
			reasoning := map[string]any{"type": "reasoning", "encrypted_content": reasoningHTTPCipher, "summary": []any{}}
			output := map[string]any{"type": "function_call_output", "call_id": reasoningHTTPCall, "output": priceResult}
			input = append(input, reasoning,
				map[string]any{"type": "function_call", "call_id": reasoningHTTPCall, "name": "emitted-probe-name", "arguments": "{}"}, output)
			if request(input).Code != http.StatusOK {
				t.Fatal("fixture did not accept paired tool result")
			}
			input = append(input, message("assistant", reasoningAmountReply(priceResult, 2)), message("user", reasoningContinuationPrompt))
			switch tc.name {
			case "equivalent_json_string":
				output["output"] = json.RawMessage(`"\u007b\"unit_price_yuan\":7}"`)
			case "summary_omitted":
				delete(reasoning, "summary")
			case "summary_null":
				reasoning["summary"] = nil
			case "summary_nonempty":
				reasoning["summary"] = []any{map[string]any{"type": "summary_text", "text": "unexpected summary"}}
			case "changed_price":
				output["output"] = `{"unit_price_yuan":8}`
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
