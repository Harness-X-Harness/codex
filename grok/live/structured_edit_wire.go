package live

import (
	"encoding/json"
	"path/filepath"
	"strings"

	"github.com/Harness-X-Harness/codex/grok/internal/integrationfixture"
)

func structuredEditIdentity(name string) bool {
	// FlatProjection.restore_response_item resolves the emitted symbol back to
	// the canonical tool name before this raw-event boundary.
	return name == "structured_edit"
}

func editArgumentsMatch(raw, path string, replaceAll bool) bool {
	args, valid := integrationfixture.ParseEditArguments(raw)
	return valid && editPathMatches(args.FilePath, path) &&
		args.OldString == editOld && args.NewString == editNew && args.ReplaceAll == replaceAll
}

// Relative tool paths are resolved in this scenario's workspace, not by basename.
// Do not claim lexical parent traversal is equivalent across possible symlinks.
func editPathMatches(candidate, target string) bool {
	if candidate == "" || !filepath.IsAbs(target) {
		return false
	}
	for _, part := range strings.Split(filepath.ToSlash(candidate), "/") {
		if part == ".." {
			return false
		}
	}
	if !filepath.IsAbs(candidate) {
		candidate = filepath.Join(filepath.Dir(target), candidate)
	}
	return filepath.Clean(candidate) == filepath.Clean(target)
}

func editOutputTexts(raw json.RawMessage) ([]string, bool) {
	if len(raw) == 0 || len(raw) > 64<<10 {
		return nil, false
	}
	var text string
	if json.Unmarshal(raw, &text) == nil {
		return []string{text}, strings.TrimSpace(text) != ""
	}
	var content []struct {
		Type string `json:"type"`
		Text string `json:"text"`
	}
	if json.Unmarshal(raw, &content) != nil || len(content) == 0 {
		return nil, false
	}
	texts := make([]string, 0, len(content))
	for _, item := range content {
		if item.Type != "input_text" || strings.TrimSpace(item.Text) == "" {
			return nil, false
		}
		texts = append(texts, item.Text)
	}
	return texts, true
}

func editOutputPresent(raw json.RawMessage) bool {
	_, valid := editOutputTexts(raw)
	return valid
}

// Compare the facts consumed by this edit scenario, not unrelated serialization.
// Invalid authority and parameters remain distinguishable and are never normalized
// into a previously admitted item. Output block boundaries are retained.
func editItemFacts(item editItem, fixture *editFixture) any {
	switch item.Type {
	case "function_call":
		var args any = item.Arguments
		if parsed, valid := integrationfixture.ParseEditArguments(item.Arguments); valid {
			if editPathMatches(parsed.FilePath, fixture.path) {
				parsed.FilePath = fixture.path
			}
			args = parsed
		}
		return []any{item.Name, item.Namespace, item.CallID, args}
	case "function_call_output":
		var output any = []any{"invalid", item.Output}
		if texts, valid := editOutputTexts(item.Output); valid {
			output = []any{"text_blocks", texts}
		}
		name := item.Name
		if structuredEditIdentity(name) {
			name = "" // The output's name is optional; call_id binds the result.
		}
		return []any{name, item.Namespace, item.CallID, output}
	case "fileChange":
		changes := append(item.Changes[:0:0], item.Changes...)
		for index := range changes {
			if editPathMatches(changes[index].Path, fixture.path) {
				changes[index].Path = fixture.path
			}
		}
		return []any{item.ID, item.Status, changes}
	case "agentMessage":
		return []any{item.ID, item.Phase, strings.TrimSpace(item.Text)}
	default:
		return item
	}
}
