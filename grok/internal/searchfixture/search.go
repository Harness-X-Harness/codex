// Package searchfixture defines the retained, bounded Web/X scenario inputs.
// It is shared by Facts HTTP and Live stdio observers, not product policy.
package searchfixture

import (
	"encoding/json"
	"errors"
	"strings"
)

// Plan returns a fresh fixed fixture. These controls never widen user policy.
func Plan(scenario string) (map[string]any, map[string]any, string, error) {
	tool := map[string]any{"type": "web_search"}
	config := map[string]any{"web_search": "live"}
	prompt := "Use hosted web search to find the current top headline on https://www.reuters.com. Reply with its headline and URL. Do not answer from memory."
	switch scenario {
	case "web":
	case "web_allowed":
		tool["filters"] = map[string]any{"allowed_domains": []string{"reuters.com"}}
		config["tools.web_search.allowed_domains"] = []string{"reuters.com"}
	case "web_excluded":
		tool["filters"] = map[string]any{"excluded_domains": []string{"example.com"}}
		config["tools.web_search.excluded_domains"] = []string{"example.com"}
	case "x", "x_window":
		tool = map[string]any{"type": "x_search"}
		prompt = "Use hosted X search to find a recent post from @xai. Reply with its text and URL. Do not answer from memory."
		if scenario == "x_window" {
			tool["from_date"], tool["to_date"] = "2026-08-01", "2026-08-15"
			config["model_providers.grok.x_search.from_date"] = "2026-08-01"
			config["model_providers.grok.x_search.to_date"] = "2026-08-15"
			prompt = "Use hosted X search to find a post from @xai in the configured August 2026 date window. Reply with its text and URL."
		}
	default:
		return nil, nil, "", errors.New("unknown hosted search fixture")
	}
	return tool, config, prompt, nil
}

// ReplayCall selects only fields admitted by the retained completed-hosted contract.
// IDs/input/action remain private in memory. Unknown hosted names never qualify.
func ReplayCall(raw json.RawMessage, scenario string) (map[string]any, bool) {
	if len(raw) > 64<<10 {
		return nil, false
	}
	var item map[string]any
	if json.Unmarshal(raw, &item) != nil || item["status"] != "completed" {
		return nil, false
	}
	text := func(key string) string { value, _ := item[key].(string); return value }
	if text("id") == "" {
		return nil, false
	}
	fields := []string{"type", "id", "status"}
	if strings.HasPrefix(scenario, "web") {
		if item["type"] != "web_search_call" {
			return nil, false
		}
		action, ok := item["action"].(map[string]any)
		if !ok {
			return nil, false
		}
		// Match the typed WebSearchAction variants: action details are optional.
		// Their absence or empty values do not invalidate a completed hosted call.
		actionFields := []string{"type"}
		switch action["type"] {
		case "search":
			actionFields = append(actionFields, "query", "queries")
			if value := action["query"]; value != nil {
				if _, ok := value.(string); !ok {
					return nil, false
				}
			}
			if value := action["queries"]; value != nil {
				queries, ok := value.([]any)
				if !ok {
					return nil, false
				}
				for _, query := range queries {
					if _, ok := query.(string); !ok {
						return nil, false
					}
				}
			}
		case "open_page":
			actionFields = append(actionFields, "url")
		case "find_in_page":
			actionFields = append(actionFields, "url", "pattern")
		default:
			return nil, false
		}
		for _, key := range actionFields {
			if key != "queries" && action[key] != nil {
				if _, ok := action[key].(string); !ok {
					return nil, false
				}
			}
		}
		admitted := make(map[string]any, len(actionFields))
		for _, key := range actionFields {
			if value, exists := action[key]; exists {
				admitted[key] = value
			}
		}
		item["action"] = admitted
		fields = append(fields, "action")
	} else {
		if item["type"] != "custom_tool_call" || text("call_id") == "" || item["namespace"] != nil {
			return nil, false
		}
		switch text("name") {
		case "x_keyword_search", "x_semantic_search", "x_user_search", "x_thread_fetch":
		default:
			return nil, false
		}
		if _, ok := item["input"].(string); !ok {
			return nil, false
		}
		fields = append(fields, "call_id", "name", "input")
	}
	replay := make(map[string]any, len(fields))
	for _, key := range fields {
		replay[key] = item[key]
	}
	return replay, true
}
