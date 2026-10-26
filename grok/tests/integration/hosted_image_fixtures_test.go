//go:build integration

package integration_test

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"sync"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
)

func assistantEvents(index int, id, text string) []any {
	added := map[string]any{"type": "message", "id": id, "role": "assistant", "phase": "final_answer", "content": []any{}}
	done := map[string]any{"type": "message", "id": id, "role": "assistant", "phase": "final_answer", "status": "completed", "content": []any{map[string]any{"type": "output_text", "text": text}}}
	return []any{
		map[string]any{"type": "response.output_item.added", "output_index": index, "item": added},
		map[string]any{"type": "response.output_text.delta", "output_index": index, "delta": text},
		map[string]any{"type": "response.output_item.done", "output_index": index, "item": done},
	}
}

func functionCallEvents(callID, name, arguments string) []any {
	added := map[string]any{"type": "function_call", "id": callID + "-item", "call_id": callID, "name": name, "arguments": ""}
	done := map[string]any{"type": "function_call", "id": callID + "-item", "call_id": callID, "name": name, "arguments": arguments, "status": "completed"}
	return []any{
		map[string]any{"type": "response.output_item.added", "output_index": 0, "item": added},
		map[string]any{"type": "response.function_call_arguments.delta", "output_index": 0, "delta": arguments},
		map[string]any{"type": "response.output_item.done", "output_index": 0, "item": done},
	}
}

type hostedNativeHTTP struct {
	mu          sync.Mutex
	unexpectedX bool
	scenario    string
	failure     string
	responses   int
	tools       []string
}

const hostedNativeReply = "Controlled fixture reply."

const hostedNativeXInput = `{"query":"fixture"}`

func hostedSearchEvents(scenario string) []any {
	if strings.HasPrefix(scenario, "web") {
		start := map[string]any{"type": "web_search_call", "id": "hosted-web", "status": "in_progress"}
		done := map[string]any{"type": "web_search_call", "id": "hosted-web", "status": "completed", "action": map[string]any{"type": "search", "query": "fixture"}}
		return []any{
			map[string]any{"type": "response.output_item.added", "output_index": 0, "item": start},
			map[string]any{"type": "response.output_item.done", "output_index": 0, "item": done},
		}
	}
	start := map[string]any{"type": "custom_tool_call", "id": "hosted-x", "call_id": "hosted-x-call", "name": "x_keyword_search", "status": "in_progress", "input": hostedNativeXInput}
	done := map[string]any{"type": "custom_tool_call", "id": "hosted-x", "call_id": "hosted-x-call", "name": "x_keyword_search", "status": "completed", "input": hostedNativeXInput}
	return []any{
		map[string]any{"type": "response.output_item.added", "output_index": 0, "item": start},
		map[string]any{"type": "response.custom_tool_call_input.done", "output_index": 0, "item_id": "hosted-x", "input": hostedNativeXInput},
		map[string]any{"type": "response.output_item.done", "output_index": 0, "item": done},
	}
}

func hostedToolPolicy(scenario string, tools []json.RawMessage) string {
	var web map[string]json.RawMessage
	count := 0
	for _, raw := range tools {
		var tool struct{ Type string }
		if json.Unmarshal(raw, &tool) != nil {
			return "tool_shape"
		}
		if tool.Type == "x_search" {
			return "retired_x_advertised"
		}
		if tool.Type == "web_search" {
			count++
			if json.Unmarshal(raw, &web) != nil {
				return "tool_shape"
			}
		}
	}
	if count != 1 {
		return "web_search_absent_or_duplicated"
	}
	switch scenario {
	case "web":
		if _, ok := web["filters"]; ok {
			return "unexpected_web_filters"
		}
	case "web_allowed", "web_excluded":
		var filters struct {
			Excluded []string `json:"excluded_domains"`
			Allowed  []string `json:"allowed_domains"`
		}
		raw, ok := web["filters"]
		if !ok || json.Unmarshal(raw, &filters) != nil {
			return "domain_filters"
		}
		if scenario == "web_allowed" {
			if len(filters.Excluded) != 0 || len(filters.Allowed) != 1 || filters.Allowed[0] != "reuters.com" {
				return "allowed_domains"
			}
		} else if len(filters.Allowed) != 0 || len(filters.Excluded) != 1 || filters.Excluded[0] != "example.com" {
			return "excluded_domains"
		}
	default:
		return "scenario"
	}
	return ""
}

func imageHistory(input []json.RawMessage) (outputs int, open, ok bool) {
	type item struct {
		Type   string `json:"type"`
		Role   string `json:"role"`
		CallID string `json:"call_id"`
	}
	items := make([]item, 0, len(input))
	lastUser := -1
	for _, raw := range input {
		var decoded item
		if json.Unmarshal(raw, &decoded) != nil {
			return 0, false, false
		}
		items = append(items, decoded)
		if decoded.Type == "message" && decoded.Role == "user" {
			lastUser = len(items) - 1
		}
	}
	pending := map[string]bool{}
	for index, decoded := range items {
		switch decoded.Type {
		case "function_call":
			if decoded.CallID != "" {
				pending[decoded.CallID] = true
			}
		case "function_call_output":
			delete(pending, decoded.CallID)
			if index > lastUser {
				outputs++
			}
		}
	}
	return outputs, len(pending) > 0, true
}

type imageNativeHTTP struct {
	mu                                          sync.Mutex
	model, failure, first, second, toolHit      string
	responses, generations, edits, recognitions int
	toolNames                                   []string
}

func imageToolName(tools []json.RawMessage) (string, []string) {
	var names []string
	var match string
	for _, raw := range tools {
		var tool struct {
			Type, Name string
		}
		if json.Unmarshal(raw, &tool) == nil && tool.Type == "x_search" {
			return "", nil
		}
		if json.Unmarshal(raw, &tool) != nil || tool.Type != "function" || tool.Name == "" {
			continue
		}
		names = append(names, tool.Name)
		if match == "" && strings.HasPrefix(tool.Name, "local__image_gen_imagegen_") {
			match = tool.Name
		}
	}
	return match, names
}

func inputHasHostedSearch(input []json.RawMessage) bool {
	for _, raw := range input {
		var item struct {
			Type string `json:"type"`
		}
		if json.Unmarshal(raw, &item) == nil && (item.Type == "web_search_call" || item.Type == "custom_tool_call") {
			return true
		}
	}
	return false
}

func (fixture *hostedNativeHTTP) reject(writer http.ResponseWriter, reason string) {
	if fixture.failure == "" {
		fixture.failure = reason
	}
	http.Error(writer, "controlled fixture rejection", http.StatusBadRequest)
}

func (fixture *imageNativeHTTP) ServeHTTP(writer http.ResponseWriter, request *http.Request) {
	fixture.mu.Lock()
	defer fixture.mu.Unlock()
	data, reason := readNativeBody(request)
	if reason != "" {
		fixture.reject(writer, reason)
		return
	}
	switch request.URL.Path {
	case "/v1/responses":
		fixture.respond(writer, data)
	case "/v1/images/generations":
		fixture.image(writer, data, false)
	case "/v1/images/edits":
		fixture.image(writer, data, true)
	default:
		fixture.reject(writer, "path")
	}
}

func (fixture *imageNativeHTTP) respond(writer http.ResponseWriter, data []byte) {
	var body struct {
		Model  string            `json:"model"`
		Stream bool              `json:"stream"`
		Store  *bool             `json:"store"`
		Tools  []json.RawMessage `json:"tools"`
		Input  []json.RawMessage `json:"input"`
	}
	if json.Unmarshal(data, &body) != nil || body.Model != fixture.model || !body.Stream || (body.Store != nil && *body.Store) {
		fixture.reject(writer, "request_contract")
		return
	}
	// Fresh recognition turns contain the image itself, with no generation
	// conversation or answer-bearing prompt. This is controlled transport proof,
	// not a claim that these synthetic pixels depict a dog.
	var attached string
	var question string
	for _, raw := range body.Input {
		var item struct {
			Type, Role string
			Content    []struct {
				Type, Text string
				ImageURL   string `json:"image_url"`
			}
		}
		if json.Unmarshal(raw, &item) != nil || item.Role != "user" {
			continue
		}
		for _, part := range item.Content {
			if part.Type == "input_image" {
				attached = part.ImageURL
			}
			if part.Type == "input_text" {
				question += part.Text
			}
		}
	}
	if attached != "" {
		if strings.Contains(strings.ToLower(question), "dog") || strings.Contains(strings.ToLower(question), "red hat") || inputHasFunctionCall(body.Input) {
			fixture.reject(writer, "recognition_prompt_leaked")
			return
		}
		reply := ""
		switch attached {
		case "data:image/png;base64," + fixture.first:
			reply = "The image shows a dog."
		case "data:image/png;base64," + fixture.second:
			reply = "The dog is wearing a red hat."
		default:
			fixture.reject(writer, "recognition_image_mismatch")
			return
		}
		fixture.responses++
		fixture.recognitions++
		events := []any{map[string]any{"type": "response.created", "response": map[string]any{"id": "recognition"}}}
		events = append(events, assistantEvents(0, "recognition-reply", reply)...)
		events = append(events, map[string]any{"type": "response.completed", "response": map[string]any{"id": "recognition"}})
		if err := writeSSE(writer, events); err != nil {
			fixture.reject(writer, "write")
		}
		return
	}
	name, names := imageToolName(body.Tools)
	if fixture.toolNames == nil {
		fixture.toolNames = names
	}
	if name == "" {
		fixture.reject(writer, "imagegen_absent")
		return
	}
	if fixture.toolHit == "" {
		fixture.toolHit = name
	} else if fixture.toolHit != name {
		fixture.reject(writer, "imagegen_changed")
		return
	}
	outputs, open, ok := imageHistory(body.Input)
	if !ok {
		fixture.reject(writer, "history_item")
		return
	}
	if open {
		fixture.reject(writer, "open_function_call")
		return
	}
	fixture.responses++
	if fixture.responses > 6 {
		fixture.reject(writer, "response_budget")
		return
	}
	var events []any
	if outputs > 0 {
		events = assistantEvents(0, fmt.Sprintf("image-reply-%d", fixture.responses), hostedNativeReply)
	} else {
		callID := "fixture-image-gen"
		arguments := `{"prompt":"blue circle"}`
		if fixture.generations > 0 {
			callID = "fixture-image-edit"
			arguments = `{"prompt":"green circle","num_last_images_to_include":1}`
		}
		events = functionCallEvents(callID, fixture.toolHit, arguments)
	}
	framed := []any{map[string]any{"type": "response.created", "response": map[string]any{"id": fmt.Sprintf("image-%d", fixture.responses)}}}
	framed = append(framed, events...)
	framed = append(framed, map[string]any{"type": "response.completed", "response": map[string]any{"id": fmt.Sprintf("image-%d", fixture.responses)}})
	if err := writeSSE(writer, framed); err != nil {
		fixture.reject(writer, "write")
	}
}

func (fixture *imageNativeHTTP) image(writer http.ResponseWriter, data []byte, edit bool) {
	var body struct {
		Model          string          `json:"model"`
		Prompt         string          `json:"prompt"`
		ResponseFormat string          `json:"response_format"`
		Image          json.RawMessage `json:"image"`
		Images         json.RawMessage `json:"images"`
	}
	if json.Unmarshal(data, &body) != nil || body.Model != "grok-imagine-image-2.0" || body.ResponseFormat != "b64_json" || strings.TrimSpace(body.Prompt) == "" {
		fixture.reject(writer, "image_request")
		return
	}
	if edit {
		if fixture.generations != 1 || fixture.edits != 0 || (len(body.Image) == 0 && len(body.Images) == 0) {
			fixture.reject(writer, "image_edit_order")
			return
		}
		fixture.edits++
		writeImage(writer, fixture.second)
		return
	}
	if fixture.generations != 0 || len(body.Image) != 0 || len(body.Images) != 0 {
		fixture.reject(writer, "image_generation_order")
		return
	}
	fixture.generations++
	writeImage(writer, fixture.first)
}

func (fixture *imageNativeHTTP) reject(writer http.ResponseWriter, reason string) {
	if fixture.failure == "" {
		fixture.failure = reason
	}
	http.Error(writer, "controlled fixture rejection", http.StatusBadRequest)
}

func (fixture *hostedNativeHTTP) ServeHTTP(writer http.ResponseWriter, request *http.Request) {
	fixture.mu.Lock()
	defer fixture.mu.Unlock()
	data, reason := readNativeBody(request)
	if reason != "" {
		fixture.reject(writer, reason)
		return
	}
	if request.URL.Path != "/v1/responses" {
		fixture.reject(writer, "path")
		return
	}
	var body struct {
		Model  string            `json:"model"`
		Stream bool              `json:"stream"`
		Store  *bool             `json:"store"`
		Tools  []json.RawMessage `json:"tools"`
		Input  []json.RawMessage `json:"input"`
	}
	if json.Unmarshal(data, &body) != nil || body.Model != providerfixture.PrimaryModel || !body.Stream || (body.Store != nil && *body.Store) {
		fixture.reject(writer, "request_contract")
		return
	}
	if reason = hostedToolPolicy(fixture.scenario, body.Tools); reason != "" {
		fixture.reject(writer, reason)
		return
	}
	fixture.responses++
	if fixture.responses > 2 {
		fixture.reject(writer, "response_budget")
		return
	}
	fixture.tools = append(fixture.tools, nativeToolLabels(body.Tools))
	responseID := fmt.Sprintf("%s-%d", fixture.scenario, fixture.responses)
	events := []any{map[string]any{"type": "response.created", "response": map[string]any{"id": responseID}}}
	index := 0
	if !inputHasHostedSearch(body.Input) {
		responseScenario := fixture.scenario
		if fixture.unexpectedX {
			responseScenario = "x"
		}
		events = append(events, hostedSearchEvents(responseScenario)...)
		index = 1
	}
	events = append(events, assistantEvents(index, responseID+"-reply", hostedNativeReply)...)
	events = append(events, map[string]any{"type": "response.completed", "response": map[string]any{"id": responseID}})
	if err := writeSSE(writer, events); err != nil {
		fixture.reject(writer, "write")
	}
}

func nativeToolLabels(tools []json.RawMessage) string {
	labels := make([]string, 0, len(tools))
	for _, raw := range tools {
		var tool struct {
			Type    string          `json:"type"`
			Name    string          `json:"name"`
			Filters json.RawMessage `json:"filters"`
			From    json.RawMessage `json:"from_date"`
			To      json.RawMessage `json:"to_date"`
		}
		if json.Unmarshal(raw, &tool) != nil {
			labels = append(labels, "invalid")
			continue
		}
		label := tool.Type
		if tool.Name != "" {
			label += ":" + tool.Name
		}
		if len(tool.Filters) > 0 {
			label += ":filters"
		}
		if len(tool.From) > 0 || len(tool.To) > 0 {
			label += ":dated"
		}
		labels = append(labels, label)
	}
	return strings.Join(labels, ",")
}

func readNativeBody(request *http.Request) ([]byte, string) {
	defer request.Body.Close()
	if request.Method != http.MethodPost || request.Header.Get("Authorization") != "Bearer fixture-key" {
		return nil, "request_contract"
	}
	data, err := io.ReadAll(io.LimitReader(request.Body, (2<<20)+1))
	if err != nil || len(data) > 2<<20 || len(data) == 0 {
		return nil, "body"
	}
	return data, ""
}

func writeImage(writer http.ResponseWriter, payload string) {
	writer.Header().Set("Content-Type", "application/json")
	fmt.Fprintf(writer, `{"data":[{"b64_json":%q}]}`, payload)
}

func writeSSE(writer http.ResponseWriter, events []any) error {
	writer.Header().Set("Content-Type", "text/event-stream")
	for _, event := range events {
		encoded, err := json.Marshal(event)
		if err != nil {
			return err
		}
		if _, err := fmt.Fprintf(writer, "data: %s\n\n", encoded); err != nil {
			return err
		}
	}
	return nil
}

func inputHasFunctionCall(input []json.RawMessage) bool {
	for _, raw := range input {
		var item struct{ Type string }
		if json.Unmarshal(raw, &item) == nil && item.Type == "function_call" {
			return true
		}
	}
	return false
}
