package integrationfixture

import (
	"encoding/json"
	"strings"
	"testing"
	"unicode/utf8"
)

func TestNativeEditFailureDetail(t *testing.T) {
	input := "Failed to write file /tmp/grok-live-owned/workspace/structured_edit_fixture.txt\n" +
		"fs sandbox helper failed with status exit status: 1: bwrap: setting up uid map: Permission denied (os error 13)\n" +
		"open /proc/1234/gid_map: Operation not permitted (os error 1)\n" +
		"GROK_\x1b[31mAPI_KEY=synthetic-do-not-publish\nAuthorization: Bearer fixture-secret\n" +
		"errno=13 exit_code=1 operation=write"
	raw, _ := json.Marshal(input)
	got := EditFailureDetail(raw)
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
	bounded := EditFailureDetail(long)
	if bounded["truncated"] != true || !strings.HasPrefix(bounded["text"].(string), "bwrap: Creating new namespace failed") || len(bounded["text"].(string)) > 4096 || !utf8.ValidString(bounded["text"].(string)) {
		t.Fatal("bounded diagnostic lost prefix, exceeded bound, or split UTF-8")
	}
	if EditFailureDetail(json.RawMessage(`{"unexpected":"object"}`))["captured"] != false || EditFailureDetail(json.RawMessage(`null`))["captured"] != false {
		t.Fatal("accepted unsupported output structure")
	}
}

func TestNativeEditResultHistoryPreservesReplay(t *testing.T) {
	for _, tc := range []struct {
		name, first, replay string
		want                bool
	}{
		{"success", `"saved file"`, `"saved file"`, true},
		{"declined", `"patch rejected by user"`, `"patch rejected by user"`, true},
		{"equivalent_string", `"saved file"`, `"saved \u0066ile"`, true},
		{"equivalent_content", `[{"type":"input_text","text":"saved file"}]`, `[ { "text": "saved file", "type": "input_text" } ]`, true},
		{"changed_string", `"saved file"`, `"different result"`, false},
		{"changed_content", `[{"type":"input_text","text":"saved file"}]`, `[{"type":"input_text","text":"different result"}]`, false},
		{"equivalent_form", `"saved file"`, `[{"type":"input_text","text":"saved file"}]`, true},
		{"equivalent_form_reverse", `[{"type":"input_text","text":"saved file"}]`, `"saved file"`, true},
		{"block_boundaries_preserved", `[{"type":"input_text","text":"saved"},{"type":"input_text","text":" file"}]`, `"saved file"`, false},
		{"at_capacity", `"` + strings.Repeat("x", (64<<10)-2) + `"`, `"` + strings.Repeat("x", (64<<10)-2) + `"`, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			var history EditResultHistory
			if !history.Accept(json.RawMessage(tc.first)) || history.Accept(json.RawMessage(tc.replay)) != tc.want || !history.Accept(json.RawMessage(tc.first)) {
				t.Fatal("paired result was lost, changed, or falsely replaced during replay")
			}
		})
	}
}

func TestNativeEditResultHistoryRejectsInvalidOutput(t *testing.T) {
	for _, tc := range []struct {
		name string
		raw  json.RawMessage
	}{
		{"missing", nil},
		{"null", json.RawMessage(`null`)},
		{"empty", json.RawMessage(`""`)},
		{"whitespace", json.RawMessage(`" \r\n"`)},
		{"malformed", json.RawMessage(`"unterminated`)},
		{"object", json.RawMessage(`{"success":true}`)},
		{"number", json.RawMessage(`1`)},
		{"empty_content", json.RawMessage(`[]`)},
		{"empty_text", json.RawMessage(`[{"type":"input_text","text":""}]`)},
		{"missing_text", json.RawMessage(`[{"type":"input_text"}]`)},
		{"other_content", json.RawMessage(`[{"type":"input_image"}]`)},
		{"unknown_field", json.RawMessage(`[{"type":"input_text","text":"saved","extra":true}]`)},
		{"oversized", json.RawMessage(`"` + strings.Repeat("x", (64<<10)-1) + `"`)},
	} {
		t.Run(tc.name, func(t *testing.T) {
			var history EditResultHistory
			valid := json.RawMessage(`"saved file"`)
			if history.Accept(tc.raw) || len(history.output) != 0 || !history.Accept(valid) {
				t.Fatal("invalid result was credited or changed first-result admission")
			}
			if history.Accept(tc.raw) || !history.Accept(valid) {
				t.Fatal("invalid replay changed the accepted result")
			}
		})
	}
}

func TestNativeStructuredEditDeclaration(t *testing.T) {
	// Mirrors flat_projection + grok_request output, with no retired hosted X declaration. Descriptions do not alter validation.
	const declaration = `{"type":"function","name":"local___structured_edit_418464e5de0f7b1e","description":"Edit an existing file","parameters":{"type":"object","properties":{"file_path":{"type":"string"},"old_string":{"type":"string"},"new_string":{"type":"string"},"replace_all":{"type":"boolean"}},"required":["file_path","old_string","new_string"],"additionalProperties":false}}`
	edit := json.RawMessage(declaration)
	hosted := json.RawMessage(`{"type":"x_search"}`)
	for _, tc := range []struct {
		name  string
		tools []json.RawMessage
		want  bool
	}{
		{"retired_hosted_rejected", []json.RawMessage{edit, hosted}, false},
		{"retired_hosted_first", []json.RawMessage{hosted, edit}, false},
		{"projected_alone", []json.RawMessage{edit}, true},
		{"plain_name", []json.RawMessage{json.RawMessage(strings.Replace(declaration, "local___structured_edit_418464e5de0f7b1e", "structured_edit", 1))}, true},
		{"different_projection", []json.RawMessage{json.RawMessage(strings.Replace(declaration, "418464e5de0f7b1e", "418464e5de0f7b1f", 1))}, true},
		{"empty_name", []json.RawMessage{json.RawMessage(strings.Replace(declaration, "local___structured_edit_418464e5de0f7b1e", "", 1))}, false},
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
			name, ok := StructuredEditToolName(tc.tools)
			var declared struct{ Name string }
			if len(tc.tools) == 1 {
				_ = json.Unmarshal(tc.tools[0], &declared)
			}
			if ok != tc.want || (ok && name != declared.Name) {
				t.Fatalf("declaration binding = %q, %v; want admitted=%v", name, ok, tc.want)
			}
		})
	}
}

func TestSameEditArgumentsBindsControlledOperation(t *testing.T) {
	const expected = `{"file_path":"structured_edit_fixture.txt","old_string":"before","new_string":"after","replace_all":false}`
	for _, tc := range []struct {
		name, actual string
		want         bool
	}{
		{"reordered_escaped_default", ` { "new_string":"a\u0066ter", "old_string":"before", "file_path":"./structured_edit_fixture.txt" } `, true},
		{"different_target", strings.Replace(expected, "structured_edit_fixture.txt", "other/structured_edit_fixture.txt", 1), false},
		{"changed_old", strings.Replace(expected, "before", "other", 1), false},
		{"changed_new", strings.Replace(expected, "after", "other", 1), false},
		{"replace_all_true", strings.Replace(expected, "false", "true", 1), false},
		{"string_boolean", strings.Replace(expected, "false", `"false"`, 1), false},
		{"null_boolean", strings.Replace(expected, "false", "null", 1), false},
		{"duplicate_key", strings.TrimSuffix(expected, "}") + `,"replace_all":false}`, false},
		{"extra_key", strings.TrimSuffix(expected, "}") + `,"extra":false}`, false},
		{"missing_key", `{"file_path":"structured_edit_fixture.txt","new_string":"after"}`, false},
		{"null_string", strings.Replace(expected, `"before"`, "null", 1), false},
		{"traversal", strings.Replace(expected, "structured_edit_fixture.txt", "sub/../structured_edit_fixture.txt", 1), false},
		{"trailing_value", expected + `{}`, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if SameEditArguments(tc.actual, expected) != tc.want {
				t.Fatal("controlled operation equality mismatch")
			}
		})
	}
}
