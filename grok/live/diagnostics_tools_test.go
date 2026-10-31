package live

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestResponseToolDiagnosticKeepsShapeNotPayload(t *testing.T) {
	dir := t.TempDir()
	d := NewDiagnostics(dir, "credential-canary")
	for _, raw := range []string{
		`{"type":"function_call","id":"item-a","call_id":"call-a","name":"spawn_agent","arguments":"{\"message\":\"PRIVATE_TASK\",\"fork_turns\":true,\"UNKNOWN_PRIVATE_FIELD\":\"credential-canary\"}"}`,
		`{"type":"function_call_output","call_id":"call-a","status":"PRIVATE_STATUS","output":"{\"success\":false,\"error\":\"PRIVATE_ERROR\"}"}`,
		`{"type":"function_call_output","call_id":"call-b","output":"ordinary PRIVATE_OUTPUT text"}`,
		`{"type":"reasoning","encrypted_content":"PRIVATE_REASONING"}`,
	} {
		recordResponseToolDiagnostic(d, "test", "thread", "turn", json.RawMessage(raw))
	}
	d.Close()
	data, err := os.ReadFile(filepath.Join(dir, "events.jsonl"))
	if err != nil {
		t.Fatal(err)
	}
	text := string(data)
	for _, want := range []string{"spawn_agent", "call-a", "known_field_types", `"fork_turns":"boolean"`, `"success":false`, `"error_present":true`, `"status":"other"`, `"kind":"text"`} {
		if !strings.Contains(text, want) {
			t.Errorf("missing diagnostic fact %q", want)
		}
	}
	for _, forbidden := range []string{"PRIVATE_TASK", "UNKNOWN_PRIVATE_FIELD", "credential-canary", "PRIVATE_STATUS", "PRIVATE_ERROR", "PRIVATE_OUTPUT", "PRIVATE_REASONING"} {
		if strings.Contains(text, forbidden) {
			t.Errorf("retained private value %q", forbidden)
		}
	}
}

func TestResponseToolDiagnosticBoundsAndJSONKinds(t *testing.T) {
	for _, test := range []struct{ raw, kind string }{
		{`null`, "null"}, {`"null"`, "null"}, {`"{\"success\":true}"`, "object"},
		{`[]`, "array"}, {`true`, "boolean"}, {`7`, "number"}, {`"plain output"`, "text"},
	} {
		if got := diagnosticJSONShape(json.RawMessage(test.raw)); got["kind"] != test.kind {
			t.Fatalf("%s: got %v", test.raw, got)
		}
	}
	if got := diagnosticJSONShape(json.RawMessage(strings.Repeat("x", 16<<10+1))); got["capture"] != "field_size_limit" {
		t.Fatal("unbounded optional field", got)
	}
	dir := t.TempDir()
	d := NewDiagnostics(dir)
	recordResponseToolEnvelopeDiagnostic(d, "test", []byte(strings.Repeat("private", 12000)))
	recordResponseToolDiagnostic(d, "test", "", "", []byte(`{"invalid":`))
	d.Close()
	data, err := os.ReadFile(filepath.Join(dir, "events.jsonl"))
	if err != nil || !strings.Contains(string(data), "envelope_size_limit") || !strings.Contains(string(data), "item_json_invalid") || strings.Contains(string(data), "private") {
		t.Fatalf("missing bounded capture status: %s %v", data, err)
	}
	// Optional sink state has no proof return value and must be safe in all states.
	recordResponseToolDiagnostic(nil, "test", "", "", nil)
	recordResponseToolEnvelopeDiagnostic(d, "test", []byte(`{"params":{"item":{"type":"function_call"}}}`))
}
