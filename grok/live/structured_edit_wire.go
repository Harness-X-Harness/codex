package live

import (
	"encoding/json"
	"io"
	"regexp"
	"strings"
)

var structuredEditWireName = regexp.MustCompile(`^local__structured_edit__[0-9a-f]{12}$`)

func structuredEditIdentity(name string) bool {
	return name == "structured_edit" || structuredEditWireName.MatchString(name)
}

type editArguments struct {
	FilePath, OldString, NewString string
	ReplaceAll                     bool
}

func editArgumentsMatch(raw, path string, replaceAll bool) bool {
	args, valid := parseEditArguments(raw)
	if args.FilePath == editFile {
		args.FilePath = path
	}
	return valid && args == (editArguments{FilePath: path, OldString: editOld, NewString: editNew, ReplaceAll: replaceAll})
}

func parseEditArguments(raw string) (editArguments, bool) {
	var parsed editArguments
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

func editOutputPresent(raw json.RawMessage) bool {
	if len(raw) == 0 || len(raw) > 64<<10 {
		return false
	}
	var text string
	if json.Unmarshal(raw, &text) == nil {
		return strings.TrimSpace(text) != ""
	}
	var content []struct {
		Type string `json:"type"`
		Text string `json:"text"`
	}
	if json.Unmarshal(raw, &content) != nil || len(content) == 0 {
		return false
	}
	for _, item := range content {
		if item.Type != "input_text" || strings.TrimSpace(item.Text) == "" {
			return false
		}
	}
	return true
}
