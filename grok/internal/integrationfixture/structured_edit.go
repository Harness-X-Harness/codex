package integrationfixture

import (
	"bytes"
	"encoding/json"
	"io"
	"path/filepath"
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
	var texts []string
	if text, ok := output.AsString(); ok {
		if strings.TrimSpace(text) == "" {
			return false
		}
		texts = []string{text}
	} else if content, ok := output.AsArray(); ok {
		if len(content) == 0 {
			return false
		}
		for _, item := range content {
			text, ok := item.AsInputText()
			if !ok || strings.TrimSpace(text.Text) == "" {
				return false
			}
			texts = append(texts, text.Text)
		}
	} else {
		return false
	}
	canonical, err := json.Marshal(texts)
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

// Retain the bounded paired text privately. String and one input_text block carry
// the same result; preserve text and multiple-block boundaries on continuation.
type EditResultHistory struct {
	mu     sync.Mutex
	output json.RawMessage
}

// The isolated scenario advertises one editor and no alternative tools. Bind its
// actual declared symbol instead of duplicating the projection algorithm here.
// This is only declaration discovery: the integration must also prove canonical
// structured_edit dispatch, exact call/result identity and actual target bytes.
func StructuredEditToolName(tools []json.RawMessage) (string, bool) {
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
			if bound != "" || strings.TrimSpace(tool.Name) == "" || schema.Type != "object" ||
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

// EditArguments are the closed exact-match operation shared by the controlled
// backend fixture and the Live raw-event observer.
type EditArguments struct {
	FilePath, OldString, NewString string
	ReplaceAll                     bool
}

// ParseEditArguments rejects ambiguity and coercion before semantic comparison.
func ParseEditArguments(raw string) (EditArguments, bool) {
	var parsed EditArguments
	if len(raw) > 4096 {
		return parsed, false
	}
	decoder := json.NewDecoder(strings.NewReader(raw))
	opening, err := decoder.Token()
	if err != nil || opening != json.Delim('{') {
		return parsed, false
	}
	args := map[string]json.RawMessage{}
	for decoder.More() {
		key, err := decoder.Token()
		if err != nil {
			return parsed, false
		}
		name, ok := key.(string)
		if !ok || args[name] != nil {
			return parsed, false
		}
		var value json.RawMessage
		if decoder.Decode(&value) != nil {
			return parsed, false
		}
		args[name] = value
	}
	if closing, err := decoder.Token(); err != nil || closing != json.Delim('}') {
		return parsed, false
	}
	if _, err := decoder.Token(); err != io.EOF {
		return parsed, false
	}
	if len(args) != 3 && len(args) != 4 {
		return parsed, false
	}
	for key, target := range map[string]*string{"file_path": &parsed.FilePath, "old_string": &parsed.OldString, "new_string": &parsed.NewString} {
		if strings.TrimSpace(string(args[key])) == "null" || json.Unmarshal(args[key], target) != nil {
			return parsed, false
		}
	}
	value, present := args["replace_all"]
	if !present {
		return parsed, len(args) == 3
	}
	switch strings.TrimSpace(string(value)) {
	case "true":
		parsed.ReplaceAll = true
	case "false":
	default:
		return parsed, false
	}
	return parsed, true
}

// SameEditArguments compares the controlled operation, not JSON formatting. The
// fixture sends relative paths; only harmless dot spelling is normalized here.
func SameEditArguments(actual, expected string) bool {
	left, leftOK := ParseEditArguments(actual)
	right, rightOK := ParseEditArguments(expected)
	if !leftOK || !rightOK {
		return false
	}
	for _, args := range []*EditArguments{&left, &right} {
		if args.FilePath == "" {
			return false
		}
		for _, part := range strings.Split(filepath.ToSlash(args.FilePath), "/") {
			if part == ".." {
				return false
			}
		}
		args.FilePath = filepath.Clean(args.FilePath)
	}
	return left == right
}
