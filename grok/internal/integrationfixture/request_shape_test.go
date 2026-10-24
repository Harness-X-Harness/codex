package integrationfixture

import (
	"encoding/json"
	"strings"
	"testing"
)

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
		shape, ok := SummarizeFirstRequest([]byte(`{"model":"grok-4.6","stream":true,"include":[],"input":[]` + tc.reasoning + `}`))
		if !ok || ContainsShapeValue(shape.ReasoningFields, "effort") != tc.present {
			t.Fatal("explicit null effort must remain distinguishable from omission")
		}
	}
}

func TestFirstRequestHistoryDeclaration(t *testing.T) {
	const valid = `{"type":"function","name":"emitted_probe_name","description":"` + HistoryProbeDescription + `","parameters":{"type":"object","properties":{},"additionalProperties":false}}`
	for _, tc := range []struct {
		name  string
		tools []json.RawMessage
		want  bool
	}{
		{"flat", []json.RawMessage{json.RawMessage(strings.Replace(valid, HistoryProbeDescription, "Call this function directly to invoke `grok_history_probe`. Do not invoke it through another tool.\\n\\n"+HistoryProbeDescription, 1))}, true},
		{"retired_hosted", []json.RawMessage{json.RawMessage(valid), json.RawMessage(`{"type":"x_search"}`)}, false},
		{"canonical", []json.RawMessage{json.RawMessage(strings.Replace(valid, "emitted_probe_name", "grok_history_probe", 1))}, true},
		{"wrong_capability", []json.RawMessage{json.RawMessage(strings.Replace(valid, HistoryProbeDescription, "unrelated capability", 1))}, false},
		{"ambiguous_name", []json.RawMessage{json.RawMessage(valid), json.RawMessage(strings.Replace(valid, HistoryProbeDescription, "unrelated capability", 1))}, false},
		{"duplicate", []json.RawMessage{json.RawMessage(valid), json.RawMessage(valid)}, false},
		{"open_schema", []json.RawMessage{json.RawMessage(strings.Replace(valid, `"additionalProperties":false`, `"additionalProperties":true`, 1))}, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if got := FirstRequestScenarioTool(tc.tools, "isolated-history"); got != tc.want {
				t.Fatalf("declaration bound=%t, want %t", got, tc.want)
			}
		})
	}
}

func TestFirstRequestShapeSummary(t *testing.T) {
	data := []byte(`{"model":"grok-4.6","stream":true,"include":["reasoning.encrypted_content"],"instructions":"PRIVATE_PROMPT","input":[{"type":"message","content":[{"text":"PRIVATE_INPUT"}]}],"tools":[{"name":"grok_history_probe","description":"PRIVATE_DESCRIPTION","parameters":{"secret":"PRIVATE_SCHEMA"}}],"reasoning":{"effort":"medium"},"prompt_cache_key":"PRIVATE_ID"}`)
	shape, ok := SummarizeFirstRequest(data)
	encoded, err := json.Marshal(shape)
	if !ok || err != nil || strings.Contains(string(encoded), "PRIVATE_") ||
		shape.Model != "grok-4.6" || shape.Effort != "medium" || shape.InstructionsBytes != len("PRIVATE_PROMPT") ||
		shape.InputTypes["message"] != 1 || !ContainsShapeValue(shape.Tools, "grok_history_probe") {
		t.Fatal("shape summary must retain structural metadata without content")
	}
	// Missing optional fields and an explicit store value are observations, not
	// claims that a particular provider deployment accepts those choices.
	for _, optional := range []string{``, `,"store":false`, `,"store":true,"reasoning":null`} {
		shape, ok := SummarizeFirstRequest([]byte(`{"model":"grok-4.7","stream":true,"include":[],"input":[]` + optional + `}`))
		if !ok || shape.Effort != "" {
			t.Fatal("optional field presence must not invent an effort or rejection")
		}
	}
	if _, ok := SummarizeFirstRequest([]byte(`{"model":true}`)); ok {
		t.Fatal("malformed required fields must be rejected")
	}
}
