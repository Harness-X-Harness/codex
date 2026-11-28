package integrationfixture

import (
	"encoding/json"
	"errors"
	"strings"
)

func ChildDeclaration(tools []json.RawMessage, operation string) (string, error) {
	var bound string
	seenNames := make(map[string]bool)
	prefix := "Call this function directly to invoke `collaboration." + operation + "`."
	for _, raw := range tools {
		var tool struct {
			Type, Name, Description string
		}
		if json.Unmarshal(raw, &tool) != nil || tool.Type == "x_search" {
			return "", errors.New("invalid declaration")
		}
		if tool.Type == "function" && tool.Name != "" {
			if seenNames[tool.Name] {
				return "", errors.New("ambiguous function name")
			}
			seenNames[tool.Name] = true
		}
		if !strings.HasPrefix(tool.Description, prefix) {
			continue
		}
		// The owning projection identifies the operation; use its emitted name.
		// Actual dispatch validates its schema. Reimplementing a partial JSON
		// Schema interpreter here rejects valid optional/union declarations.
		if tool.Type != "function" || bound != "" || tool.Name == "" {
			return "", errors.New("ambiguous or invalid collaboration declaration")
		}
		bound = tool.Name
	}
	if bound == "" {
		return "", errors.New("direct collaboration declaration absent")
	}
	return bound, nil
}
