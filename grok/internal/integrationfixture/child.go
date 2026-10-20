package integrationfixture

import (
	"encoding/json"
	"errors"
	"reflect"
	"strings"
)

func ChildDeclaration(tools []json.RawMessage, operation string) (string, error) {
	var bound string
	prefix := "Call this function directly to invoke `collaboration." + operation + "`."
	for _, raw := range tools {
		var tool struct {
			Type, Name, Description string
			Parameters              json.RawMessage
		}
		if json.Unmarshal(raw, &tool) != nil || tool.Type == "x_search" {
			return "", errors.New("invalid declaration")
		}
		if !strings.HasPrefix(tool.Description, prefix) {
			continue
		}
		var schema struct {
			Type                 string
			Properties           map[string]struct{ Type string }
			Required             []string
			AdditionalProperties *bool
		}
		if json.Unmarshal(tool.Parameters, &schema) != nil {
			return "", errors.New("unsupported collaboration schema")
		}
		required := []string{"task_name", "message"}
		if operation == "wait_agent" {
			required = nil
		}
		if tool.Type != "function" || bound != "" || tool.Name == "" ||
			schema.Type != "object" || schema.AdditionalProperties == nil || *schema.AdditionalProperties ||
			!reflect.DeepEqual(schema.Required, required) {
			return "", errors.New("unsupported collaboration declaration")
		}
		for name, property := range schema.Properties {
			if operation == "wait_agent" {
				if name != "timeout_ms" || property.Type != "number" {
					return "", errors.New("unsupported wait arguments")
				}
			} else if property.Type != "string" ||
				(name != "task_name" && name != "message" && name != "fork_turns" && name != "agent_type" && name != "model" && name != "reasoning_effort") {
				return "", errors.New("unsupported spawn arguments")
			}
		}
		if operation == "wait_agent" && len(schema.Properties) != 1 ||
			operation == "spawn_agent" && (schema.Properties["task_name"].Type != "string" ||
				schema.Properties["message"].Type != "string" || schema.Properties["fork_turns"].Type != "string") {
			return "", errors.New("missing collaboration arguments")
		}
		bound = tool.Name
	}
	if bound == "" {
		return "", errors.New("direct collaboration declaration absent")
	}
	return bound, nil
}
