package integrationfixture

import (
	"encoding/json"
	"sort"
	"strings"
)

func ContainsShapeValue(tools []string, want string) bool {
	for _, tool := range tools {
		if tool == want {
			return true
		}
	}
	return false
}

// HistoryProbeDescription is the controlled capability registered by this
// scenario. Its emitted function name belongs to the product's tool projection.
const HistoryProbeDescription = "Check this shop's current price list and return the apple price in yuan per kilogram."

// HistoryProbeDeclaration identifies the capability, not schema validity.
// The actual tool call and returned price prove callability in the history run.
func HistoryProbeDeclaration(tools []json.RawMessage) (string, bool) {
	var bound string
	seenNames := make(map[string]bool)
	for _, raw := range tools {
		var tool struct{ Type, Name, Description string }
		if json.Unmarshal(raw, &tool) != nil || tool.Type == "x_search" {
			return "", false
		}
		if tool.Type == "function" && tool.Name != "" {
			if seenNames[tool.Name] {
				return "", false
			}
			seenNames[tool.Name] = true
		}
		// The flat projection prepends routing instructions to this owned text.
		if tool.Description != HistoryProbeDescription && !strings.HasSuffix(tool.Description, "\n\n"+HistoryProbeDescription) {
			continue
		}
		if tool.Type != "function" || tool.Name == "" || bound != "" {
			return "", false
		}
		bound = tool.Name
	}
	return bound, bound != ""
}

func FirstRequestScenarioTool(tools []json.RawMessage, profile string) bool {
	if profile != "isolated-edit" {
		_, ok := HistoryProbeDeclaration(tools)
		return ok
	}
	_, ok := StructuredEditToolName(tools)
	return ok
}

type FirstRequestShape struct {
	Model             string         `json:"model"`
	Stream            bool           `json:"stream"`
	Store             *bool          `json:"store_value,omitempty"`
	Fields            []string       `json:"fields"`
	Effort            string         `json:"effort,omitempty"`
	ReasoningFields   []string       `json:"reasoning_fields"`
	Include           []string       `json:"include"`
	InstructionsBytes int            `json:"instructions_bytes"`
	Tools             []string       `json:"tools"`
	ToolTypes         map[string]int `json:"tool_types"`
	InputTypes        map[string]int `json:"input_types"`
}

func SummarizeFirstRequest(data []byte) (FirstRequestShape, bool) {
	var body map[string]json.RawMessage
	shape := FirstRequestShape{InputTypes: map[string]int{}, ToolTypes: map[string]int{}}
	if json.Unmarshal(data, &body) != nil || len(body) > 32 {
		return shape, false
	}
	for key := range body {
		shape.Fields = append(shape.Fields, key)
	}
	sort.Strings(shape.Fields)
	var instructions string
	var reasoning map[string]json.RawMessage
	var tools []struct {
		Name string `json:"name"`
		Type string `json:"type"`
	}
	var input []struct {
		Type string `json:"type"`
	}
	if json.Unmarshal(body["model"], &shape.Model) != nil || json.Unmarshal(body["stream"], &shape.Stream) != nil ||
		json.Unmarshal(body["include"], &shape.Include) != nil || json.Unmarshal(body["input"], &input) != nil || len(input) > 128 {
		return shape, false
	}
	if raw, ok := body["instructions"]; ok && json.Unmarshal(raw, &instructions) != nil {
		return shape, false
	}
	shape.InstructionsBytes = len(instructions)
	if raw, ok := body["store"]; ok && json.Unmarshal(raw, &shape.Store) != nil {
		return shape, false
	}
	if raw, ok := body["reasoning"]; ok {
		if json.Unmarshal(raw, &reasoning) != nil {
			return shape, false
		}
		for key := range reasoning {
			shape.ReasoningFields = append(shape.ReasoningFields, key)
		}
		sort.Strings(shape.ReasoningFields)
		if raw, ok := reasoning["effort"]; ok && string(raw) != "null" && json.Unmarshal(raw, &shape.Effort) != nil {
			return shape, false
		}
	}
	if raw, ok := body["tools"]; ok {
		if json.Unmarshal(raw, &tools) != nil || len(tools) > 128 {
			return shape, false
		}
		for _, tool := range tools {
			shape.ToolTypes[tool.Type]++
			if tool.Name != "" {
				shape.Tools = append(shape.Tools, tool.Name)
			}
		}
		sort.Strings(shape.Tools)
	}
	for _, item := range input {
		shape.InputTypes[item.Type]++
	}
	return shape, true
}
