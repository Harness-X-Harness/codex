package integrationfixture

import (
	"encoding/json"
	"sort"
)

func ContainsShapeValue(tools []string, want string) bool {
	for _, tool := range tools {
		if tool == want {
			return true
		}
	}
	return false
}

func FirstRequestScenarioTool(tools []json.RawMessage, profile string) bool {
	// flat_wire_name("function", ToolName::plain("grok_history_probe")),
	// owned by codex-rs/tools/src/flat_projection.rs, not the canonical name.
	want := "local___grok_history_probe_5c6dab5ce2f3142e"
	if profile == "isolated-edit" {
		// The external-package StructuredEditToolName test owns the
		// complete edit declaration; here we check only its same wire identity.
		want = "local___structured_edit_418464e5de0f7b1e"
	}
	count := 0
	for _, raw := range tools {
		var tool struct {
			Type, Name string
			Parameters struct {
				Type                 string
				Properties           map[string]json.RawMessage
				Required             []string
				AdditionalProperties *bool
			}
		}
		if json.Unmarshal(raw, &tool) != nil || tool.Type == "x_search" {
			return false
		}
		if tool.Name != want {
			continue
		}
		count++
		if tool.Type != "function" {
			return false
		}
		if profile == "isolated-edit" {
			continue
		}
		if tool.Parameters.Type != "object" || len(tool.Parameters.Properties) != 0 ||
			len(tool.Parameters.Required) != 0 || tool.Parameters.AdditionalProperties == nil || *tool.Parameters.AdditionalProperties {
			return false
		}
	}
	return count == 1
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
