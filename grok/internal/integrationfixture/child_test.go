package integrationfixture

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestShippedChildDeclarationBinding(t *testing.T) {
	const declaration = `{"type":"function","name":"declared_wait_name","description":"Call this function directly to invoke ` + "`collaboration.wait_agent`" + `.","parameters":{"type":"object","properties":{"timeout_ms":{"type":"number"}},"additionalProperties":false}}`
	unrelated := json.RawMessage(`{"type":"function","name":"unrelated","description":"Unrelated shipped capability","parameters":{"type":"object","properties":{"value":{"type":["string","null"]}}}}`)
	for _, tc := range []struct {
		name  string
		tools []json.RawMessage
		want  bool
	}{
		{"retired_x", []json.RawMessage{json.RawMessage(declaration), json.RawMessage(`{"type":"x_search"}`)}, false},
		{"unrelated_union", []json.RawMessage{unrelated, json.RawMessage(declaration)}, true},
		{"matched_union", []json.RawMessage{json.RawMessage(strings.Replace(declaration, `"type":"number"`, `"type":["number","null"]`, 1))}, true},
		{"wrong_operation", []json.RawMessage{json.RawMessage(strings.Replace(declaration, "wait_agent", "spawn_agent", 1))}, false},
		{"ambiguous_name", []json.RawMessage{json.RawMessage(declaration), json.RawMessage(strings.Replace(declaration, "wait_agent", "spawn_agent", 1))}, false},
		{"duplicate_target", []json.RawMessage{json.RawMessage(declaration), json.RawMessage(declaration)}, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			name, err := ChildDeclaration(tc.tools, "wait_agent")
			if (err == nil) != tc.want || (tc.want && name != "declared_wait_name") {
				t.Fatalf("declaration boundary: admitted=%t name=%q error=%v", err == nil, name, err)
			}
		})
	}
}
