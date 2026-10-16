package live

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
)

// This is a controlled first-request diagnostic, not real-provider acceptance.
// A synthetic denial ends every case before tool execution or history replay.
func testNativeFirstRequestShape(t *testing.T) {
	subject := nativeInternalSubject(t)
	for _, model := range []string{providerfixture.PinnedModel, providerfixture.PrimaryModel} {
		profiles := []string{"isolated-history", "shipped-history", "isolated-edit"}
		if model == providerfixture.PinnedModel {
			profiles = append(profiles, "isolated-history-no-effort")
		}
		for _, profile := range profiles {
			t.Run(model+"/"+profile, func(t *testing.T) {
				var mu sync.Mutex
				requests := 0
				var attempts []firstRequestAttempt
				valid := true
				backend := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
					defer r.Body.Close()
					mu.Lock()
					defer mu.Unlock()
					requests++
					data, err := io.ReadAll(io.LimitReader(r.Body, (1<<20)+1))
					// Grok's existing sampling retry budget is one. This fixture
					// does not change it or submit another turn to obtain success.
					if requests > 2 {
						valid = false
					} else {
						attempt := firstRequestAttempt{Method: "other", Path: "other"}
						if r.Method == "POST" {
							attempt.Method = "POST"
						}
						if r.URL.Path == "/v1/responses" {
							attempt.Path = "/v1/responses"
						}
						if attempt.Method == "other" || attempt.Path == "other" || r.Header.Get("Authorization") != "Bearer fixture-key" {
							valid = false
						}
						if err != nil || len(data) > 1<<20 {
							valid = false
						} else if got, ok := summarizeFirstRequest(data); !ok {
							valid = false
						} else {
							attempt.Shape = got
							var body struct{ Tools []json.RawMessage }
							if json.Unmarshal(data, &body) != nil || !firstRequestScenarioTool(body.Tools, profile) {
								valid = false
							}
						}
						attempts = append(attempts, attempt)
					}
					w.Header().Set("Content-Type", "application/json")
					w.WriteHeader(http.StatusForbidden)
					_, _ = io.WriteString(w, `{"error":{"message":"controlled fixture denial","type":"permission_error","code":"fixture_denied"}}`)
				}))
				defer backend.Close()
				options := Options{Subject: subject, Model: model, BaseURL: backend.URL + "/v1", APIKey: "fixture-key"}
				if profile == "shipped-history" {
					options.shippedProfile = true
					options.fixtureReady = func(home string) error {
						return substituteShippedEndpoint(home, options.BaseURL)
					}
				}
				ctx, cancel := context.WithTimeout(context.Background(), time.Minute)
				defer cancel()
				var evidence Evidence
				var runErr error
				if profile == "isolated-edit" {
					// The pinned public wrapper selects this same one-turn fixture.
					got, err := runStructuredEdit(ctx, options, editFixture{})
					evidence, runErr = got.Evidence, err
				} else if profile == "isolated-history-no-effort" {
					evidence, runErr = ReasoningHistoryWithoutEffort(ctx, options)
				} else {
					evidence, runErr = ReasoningHistory(ctx, options)
				}
				mu.Lock()
				defer mu.Unlock()
				failure := DescribeFailure(runErr)
				encoded, _ := json.Marshal(attempts)
				// Never emit raw request/response bodies, prompts, headers or IDs.
				t.Logf("controlled first request: source=%s harness=%s target=%s executable_sha256=%s profile=%s requests=%d attempts=%s failure=%+v",
					subject.SourceSHA, subject.HarnessSHA, subject.Target, subject.SHA256, profile, requests, encoded, failure)
				if evidence.CleanupPending || runErr == nil || !valid || requests < 1 || requests > 2 || len(attempts) != requests ||
					!evidence.Bound || evidence.Completed || evidence.Turns != 1 || evidence.Processes != 1 || evidence.Threads != 1 {
					t.Fatal("controlled denial must remain unsuccessful in one bound turn within the existing sampling attempt budget")
				}
				if profile != "isolated-edit" && (failure.Code != "turn_unsuccessful" || failure.TerminalStatus != "failed" || failure.HTTPStatus != 403) {
					t.Error("history observer lost its bound failure diagnostics")
				}
				if profile == "isolated-edit" && (runErr.Error() != "live: edit turn did not complete successfully" || failure.Code != "turn_unsuccessful") {
					t.Error("edit request-shape proof must reach the rejected terminal, not a timeout or unrelated error")
				}
				for _, attempt := range attempts {
					shape := attempt.Shape
					if shape.Model != model || !shape.Stream || !containsShapeValue(shape.Include, "reasoning.encrypted_content") {
						t.Error("model, streaming or encrypted-reasoning request contract changed")
					}
					if shape.Store == nil || *shape.Store {
						t.Error("server-side response storage was not explicitly disabled")
					}
					if profile == "isolated-history-no-effort" {
						if containsShapeValue(shape.ReasoningFields, "effort") {
							t.Error("omitted history effort was reintroduced on the wire")
						}
					} else if profile != "isolated-edit" && shape.Effort != "medium" {
						t.Error("explicit history effort was not preserved")
					}
				}
				// Other presence/defaults are observations, not guessed backend rules.
			})
		}
	}
}

type firstRequestAttempt struct {
	Method string            `json:"method"`
	Path   string            `json:"path"`
	Shape  firstRequestShape `json:"shape"`
}

type firstRequestShape struct {
	Model             string         `json:"model"`
	Stream            bool           `json:"stream"`
	Store             *bool          `json:"store_value,omitempty"`
	Fields            []string       `json:"fields"`
	Effort            string         `json:"effort,omitempty"`
	ReasoningFields   []string       `json:"reasoning_fields"`
	Include           []string       `json:"include"`
	InstructionsBytes int            `json:"instructions_bytes"`
	Tools             []string       `json:"tools"`
	ToolTypes         map[string]int `json:"tool_types"`
	InputTypes        map[string]int `json:"input_types"`
}

func summarizeFirstRequest(data []byte) (firstRequestShape, bool) {
	var body map[string]json.RawMessage
	shape := firstRequestShape{InputTypes: map[string]int{}, ToolTypes: map[string]int{}}
	if json.Unmarshal(data, &body) != nil || len(body) > 32 {
		return shape, false
	}
	for key := range body {
		shape.Fields = append(shape.Fields, key)
	}
	sort.Strings(shape.Fields)
	var instructions string
	var reasoning map[string]json.RawMessage
	var tools []struct {
		Name string `json:"name"`
		Type string `json:"type"`
	}
	var input []struct {
		Type string `json:"type"`
	}
	if json.Unmarshal(body["model"], &shape.Model) != nil || json.Unmarshal(body["stream"], &shape.Stream) != nil ||
		json.Unmarshal(body["include"], &shape.Include) != nil || json.Unmarshal(body["input"], &input) != nil || len(input) > 128 {
		return shape, false
	}
	if raw, ok := body["instructions"]; ok && json.Unmarshal(raw, &instructions) != nil {
		return shape, false
	}
	shape.InstructionsBytes = len(instructions)
	if raw, ok := body["store"]; ok && json.Unmarshal(raw, &shape.Store) != nil {
		return shape, false
	}
	if raw, ok := body["reasoning"]; ok {
		if json.Unmarshal(raw, &reasoning) != nil {
			return shape, false
		}
		for key := range reasoning {
			shape.ReasoningFields = append(shape.ReasoningFields, key)
		}
		sort.Strings(shape.ReasoningFields)
		if raw, ok := reasoning["effort"]; ok && string(raw) != "null" && json.Unmarshal(raw, &shape.Effort) != nil {
			return shape, false
		}
	}
	if raw, ok := body["tools"]; ok {
		if json.Unmarshal(raw, &tools) != nil || len(tools) > 128 {
			return shape, false
		}
		for _, tool := range tools {
			shape.ToolTypes[tool.Type]++
			if tool.Name != "" {
				shape.Tools = append(shape.Tools, tool.Name)
			}
		}
		sort.Strings(shape.Tools)
	}
	for _, item := range input {
		shape.InputTypes[item.Type]++
	}
	return shape, true
}

func firstRequestScenarioTool(tools []json.RawMessage, profile string) bool {
	// flat_wire_name("function", ToolName::plain("grok_history_probe")),
	// owned by codex-rs/tools/src/flat_projection.rs, not the canonical name.
	want := "local___grok_history_probe_5c6dab5ce2f3142e"
	if profile == "isolated-edit" {
		// The external-package nativeStructuredEditToolName test owns the
		// complete edit declaration; here we check only its same wire identity.
		want = "local___structured_edit_418464e5de0f7b1e"
	}
	count := 0
	for _, raw := range tools {
		var tool struct {
			Type, Name string
			Parameters struct {
				Type                 string
				Properties           map[string]json.RawMessage
				Required             []string
				AdditionalProperties *bool
			}
		}
		if json.Unmarshal(raw, &tool) != nil {
			return false
		}
		if tool.Name != want {
			continue
		}
		count++
		if tool.Type != "function" {
			return false
		}
		if profile == "isolated-edit" {
			continue
		}
		if tool.Parameters.Type != "object" || len(tool.Parameters.Properties) != 0 ||
			len(tool.Parameters.Required) != 0 || tool.Parameters.AdditionalProperties == nil || *tool.Parameters.AdditionalProperties {
			return false
		}
	}
	return count == 1
}

func containsShapeValue(tools []string, want string) bool {
	for _, tool := range tools {
		if tool == want {
			return true
		}
	}
	return false
}

func TestFirstRequestShapeSummary(t *testing.T) {
	data := []byte(`{"model":"grok-4.6","stream":true,"include":["reasoning.encrypted_content"],"instructions":"PRIVATE_PROMPT","input":[{"type":"message","content":[{"text":"PRIVATE_INPUT"}]}],"tools":[{"name":"grok_history_probe","description":"PRIVATE_DESCRIPTION","parameters":{"secret":"PRIVATE_SCHEMA"}}],"reasoning":{"effort":"medium"},"prompt_cache_key":"PRIVATE_ID"}`)
	shape, ok := summarizeFirstRequest(data)
	encoded, err := json.Marshal(shape)
	if !ok || err != nil || strings.Contains(string(encoded), "PRIVATE_") ||
		shape.Model != "grok-4.6" || shape.Effort != "medium" || shape.InstructionsBytes != len("PRIVATE_PROMPT") ||
		shape.InputTypes["message"] != 1 || !containsShapeValue(shape.Tools, historyTool) {
		t.Fatal("shape summary must retain structural metadata without content")
	}
	// Missing optional fields and an explicit store value are observations, not
	// claims that a particular provider deployment accepts those choices.
	for _, optional := range []string{``, `,"store":false`, `,"store":true,"reasoning":null`} {
		shape, ok := summarizeFirstRequest([]byte(`{"model":"grok-4.7","stream":true,"include":[],"input":[]` + optional + `}`))
		if !ok || shape.Effort != "" {
			t.Fatal("optional field presence must not invent an effort or rejection")
		}
	}
	if _, ok := summarizeFirstRequest([]byte(`{"model":true}`)); ok {
		t.Fatal("malformed required fields must be rejected")
	}
}

func TestFirstRequestEffortPresence(t *testing.T) {
	for _, tc := range []struct {
		reasoning string
		present   bool
	}{
		{``, false},
		{`,"reasoning":null`, false},
		{`,"reasoning":{"effort":null}`, true},
		{`,"reasoning":{"effort":"medium"}`, true},
	} {
		shape, ok := summarizeFirstRequest([]byte(`{"model":"grok-4.6","stream":true,"include":[],"input":[]` + tc.reasoning + `}`))
		if !ok || containsShapeValue(shape.ReasoningFields, "effort") != tc.present {
			t.Fatal("explicit null effort must remain distinguishable from omission")
		}
	}
}

func TestFirstRequestHistoryDeclaration(t *testing.T) {
	const valid = `{"type":"function","name":"local___grok_history_probe_5c6dab5ce2f3142e","parameters":{"type":"object","properties":{},"additionalProperties":false}}`
	for _, tc := range []struct {
		name  string
		tools []json.RawMessage
		want  bool
	}{
		{"flat", []json.RawMessage{json.RawMessage(valid)}, true},
		{"hosted", []json.RawMessage{json.RawMessage(valid), json.RawMessage(`{"type":"x_search"}`)}, true},
		{"canonical", []json.RawMessage{json.RawMessage(strings.Replace(valid, "local___grok_history_probe_5c6dab5ce2f3142e", historyTool, 1))}, false},
		{"duplicate", []json.RawMessage{json.RawMessage(valid), json.RawMessage(valid)}, false},
		{"open_schema", []json.RawMessage{json.RawMessage(strings.Replace(valid, `"additionalProperties":false`, `"additionalProperties":true`, 1))}, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if got := firstRequestScenarioTool(tc.tools, "isolated-history"); got != tc.want {
				t.Fatalf("declaration bound=%t, want %t", got, tc.want)
			}
		})
	}
}
