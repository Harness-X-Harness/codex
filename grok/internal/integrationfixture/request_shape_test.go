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
	const valid = `{"type":"function","name":"local___grok_history_probe_5c6dab5ce2f3142e","parameters":{"type":"object","properties":{},"additionalProperties":false}}`
	for _, tc := range []struct {
		name  string
		tools []json.RawMessage
		want  bool
	}{
		{"flat", []json.RawMessage{json.RawMessage(valid)}, true},
		{"retired_hosted", []json.RawMessage{json.RawMessage(valid), json.RawMessage(`{"type":"x_search"}`)}, false},
		{"canonical", []json.RawMessage{json.RawMessage(strings.Replace(valid, "local___grok_history_probe_5c6dab5ce2f3142e", "grok_history_probe", 1))}, false},
		{"duplicate", []json.RawMessage{json.RawMessage(valid), json.RawMessage(valid)}, false},
		{"open_schema", []json.RawMessage{json.RawMessage(strings.Replace(valid, `"additionalProperties":false`, `"additionalProperties":true`, 1))}, false},
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
