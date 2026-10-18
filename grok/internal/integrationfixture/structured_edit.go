package integrationfixture

import (
	"bytes"
	"encoding/json"
	"regexp"
	"strings"
	"sync"
	"unicode"
	"unicode/utf8"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func (history *EditResultHistory) Accept(raw json.RawMessage) bool {
	if len(raw) == 0 || len(raw) > 64<<10 {
		return false
	}
	var output protocolv2.FunctionCallOutputBody
	if json.Unmarshal(raw, &output) != nil {
		return false
	}
	if text, ok := output.AsString(); ok {
		if strings.TrimSpace(text) == "" {
			return false
		}
	} else if content, ok := output.AsArray(); ok {
		if len(content) == 0 {
			return false
		}
		for _, item := range content {
			text, ok := item.AsInputText()
			if !ok || strings.TrimSpace(text.Text) == "" {
				return false
			}
		}
	} else {
		return false
	}
	canonical, err := json.Marshal(output)
	if err != nil {
		return false
	}
	history.mu.Lock()
	defer history.mu.Unlock()
	if history.output == nil {
		history.output = canonical
		return true
	}
	return bytes.Equal(history.output, canonical)
}

// Only used for the owned synthetic edit in the loopback fixture above. Real
// backend scenarios never retain tool-output text. Keep error operations and
// numeric errno/status while removing paths and credential/environment lines.
func EditFailureDetail(raw json.RawMessage) map[string]any {
	var text string
	if !strings.HasPrefix(strings.TrimSpace(string(raw)), "\"") || json.Unmarshal(raw, &text) != nil {
		return map[string]any{"format": "unsupported", "captured": false}
	}
	const limit = 4096
	truncated := false
	// The HTTP body is already capped at 1 MiB. Sanitize before output truncation.
	ansi := regexp.MustCompile("\x1b\\[[0-9;]*[A-Za-z]")
	text = ansi.ReplaceAllString(text, "")
	text = strings.Map(func(r rune) rune {
		if unicode.IsControl(r) && r != '\n' && r != '\t' {
			return -1
		}
		return r
	}, text)
	credentials := regexp.MustCompile(`\b[A-Z][A-Z0-9_]*=|(?i:\b(?:authorization|bearer|api[_-]?key|access[_-]?token|token|password|secret)\b\s*[:= ]|gh[pousr]_[A-Za-z0-9]+|github_pat_[A-Za-z0-9_]+|sk-[A-Za-z0-9]+)`)
	paths := regexp.MustCompile(`/[^\s"'<>\\:]+`)
	lines := strings.Split(text, "\n")
	filtered := 0
	for i, line := range lines {
		if credentials.MatchString(line) {
			lines[i] = "[filtered credential/environment line]"
			filtered++
			continue
		}
		lines[i] = paths.ReplaceAllStringFunc(line, func(path string) string {
			for _, suffix := range []string{"/uid_map", "/gid_map", "/setgroups", "/codex-linux-sandbox", "/structured_edit_fixture.txt"} {
				if strings.HasSuffix(path, suffix) {
					return "<path:" + strings.TrimPrefix(suffix, "/") + ">"
				}
			}
			return "<path>"
		})
	}
	text = strings.Join(lines, "\n")
	if len(text) > limit {
		truncated = true
		text = text[:limit]
		for !utf8.ValidString(text) {
			text = text[:len(text)-1]
		}
	}
	return map[string]any{"format": "json_string", "captured": true, "text": text, "truncated": truncated, "filtered_lines": filtered}
}

// Retain only the bounded paired result privately. Canonical SDK serialization
// permits equivalent JSON encoding while preserving the result in continuation.
type EditResultHistory struct {
	mu     sync.Mutex
	output json.RawMessage
}

// This fixture is tied to the plain structured_edit capability in the identified
// Grok package. The name is flat_wire_name("function", ToolName::plain(...));
// return the declaration's name so the response uses the actual wire identity.
func StructuredEditToolName(tools []json.RawMessage) (string, bool) {
	const projectedName = "local___structured_edit_418464e5de0f7b1e"
	var bound string
	for _, raw := range tools {
		var tool struct {
			Type, Name string
			Parameters struct {
				Type                 string
				Properties           map[string]struct{ Type string }
				Required             []string
				AdditionalProperties *bool
			}
		}
		if json.Unmarshal(raw, &tool) != nil {
			return "", false
		}
		switch tool.Type {
		case "function":
			schema := tool.Parameters
			if bound != "" || tool.Name != projectedName || schema.Type != "object" ||
				schema.AdditionalProperties == nil || *schema.AdditionalProperties ||
				len(schema.Required) != 3 || len(schema.Properties) < 4 || len(schema.Properties) > 5 {
				return "", false
			}
			required := map[string]bool{"file_path": false, "old_string": false, "new_string": false}
			for _, name := range schema.Required {
				seen, known := required[name]
				if !known || seen {
					return "", false
				}
				required[name] = true
			}
			for name, property := range schema.Properties {
				expected := "string"
				switch name {
				case "file_path", "old_string", "new_string", "environment_id":
				case "replace_all":
					expected = "boolean"
				default:
					return "", false
				}
				if property.Type != expected {
					return "", false
				}
			}
			for _, name := range []string{"file_path", "old_string", "new_string", "replace_all"} {
				if _, exists := schema.Properties[name]; !exists {
					return "", false
				}
			}
			bound = tool.Name
		default:
			return "", false
		}
	}
	return bound, bound != ""
}
