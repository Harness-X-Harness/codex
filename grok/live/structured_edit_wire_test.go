package live

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestStructuredEditArgumentsRejectMalformedReplaceAll(t *testing.T) {
	path := "/workspace/structured_edit_fixture.txt"
	args := map[string]any{"file_path": editFile, "old_string": editOld, "new_string": editNew}
	body, _ := json.Marshal(args)
	if !editArgumentsMatch(string(body), path, false) || editArgumentsMatch(string(body), path, true) {
		t.Fatal("omitted replace_all was not default false")
	}
	for _, value := range []any{nil, "true", 1, []any{true}, map[string]bool{"value": true}} {
		args["replace_all"] = value
		body, _ = json.Marshal(args)
		if editArgumentsMatch(string(body), path, true) || editArgumentsMatch(string(body), path, false) {
			t.Fatal("malformed replace_all accepted")
		}
	}
	args["replace_all"] = true
	body, _ = json.Marshal(args)
	if !editArgumentsMatch(string(body), path, true) || editArgumentsMatch(string(body), path, false) {
		t.Fatal("JSON true was not required")
	}
	for _, invalid := range []string{string(body) + ` {}`, strings.TrimSuffix(string(body), "}") + `,"replace_all":true}`, strings.Repeat(" ", 4096) + string(body)} {
		if editArgumentsMatch(invalid, path, true) {
			t.Fatal("ambiguous arguments accepted")
		}
	}
}
