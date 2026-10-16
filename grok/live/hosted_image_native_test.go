package live_test

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

// Controlled local backends. A successful run shows that this frozen product
// finishes the hosted-search and image-pinned turns when the provider admits
// the expected items. It does not replay a real provider.
func testNativeHostedSearchFixture(t *testing.T) {
	subject := nativeSubject(t)
	for _, scenario := range []string{"web", "web_excluded", "x", "x_window"} {
		t.Run(scenario, func(t *testing.T) {
			fixture := &hostedNativeHTTP{scenario: scenario}
			backend := httptest.NewServer(fixture)
			defer backend.Close()
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
			defer cancel()
			options := live.Options{Subject: subject, Model: providerfixture.PrimaryModel, BaseURL: backend.URL + "/v1", APIKey: "fixture-key"}
			got, err := live.HostedSearch(ctx, options, scenario)
			fixture.mu.Lock()
			defer fixture.mu.Unlock()
			encoded, _ := json.Marshal(got)
			t.Logf("controlled %s: evidence=%s responses=%d failure=%s tools=%s", scenario, encoded, fixture.responses, fixture.failure, strings.Join(fixture.tools, " | "))
			if err != nil || fixture.failure != "" || fixture.responses != 2 || strings.Contains(string(encoded), "PRIVATE_") {
				t.Fatalf("native hosted search: %+v; %v", got, err)
			}
			if !got.Completed || !got.FirstCompleted || !got.FollowupCompleted || got.Scenario != scenario || got.CanonicalCalls != 1 || got.SettledTurns != 2 || got.Turns != 2 || got.Processes != 1 || got.Initializations != 1 || got.Threads != 1 || got.Model != options.Model {
				t.Fatal("native hosted search evidence was not proven")
			}
		})
	}
}

func testNativeImagePinnedFixture(t *testing.T) {
	subject := nativeSubject(t)
	first := tinyPNG(t, color.NRGBA{R: 0, G: 0, B: 255, A: 255})
	second := tinyPNG(t, color.NRGBA{R: 0, G: 255, B: 0, A: 255})
	if first == second {
		t.Fatal("png fixtures are not distinct")
	}
	fixture := &imageNativeHTTP{model: providerfixture.PinnedModel, first: first, second: second}
	backend := httptest.NewServer(fixture)
	defer backend.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	options := live.Options{Subject: subject, Model: providerfixture.PinnedModel, BaseURL: backend.URL + "/v1", APIKey: "fixture-key"}
	got, err := live.ImageGenerationEdit(ctx, options)
	fixture.mu.Lock()
	defer fixture.mu.Unlock()
	encoded, _ := json.Marshal(got)
	t.Logf("controlled image-pinned: evidence=%s responses=%d generations=%d edits=%d failure=%s tools=%s", encoded, fixture.responses, fixture.generations, fixture.edits, fixture.failure, strings.Join(fixture.toolNames, ","))
	if err != nil || fixture.failure != "" || fixture.responses != 4 || fixture.generations != 1 || fixture.edits != 1 || strings.Contains(string(encoded), "PRIVATE_") {
		t.Fatalf("native image edit: %+v; %v", got, err)
	}
	if !got.Completed || !got.FirstCompleted || !got.TurnCompleted || !got.Bound || !got.HistorySelected || got.Images != 2 || got.Artifacts != 2 || got.ToolCalls != 2 || got.FirstImageSHA256 == "" || got.EditedImageSHA256 == "" || got.FirstImageSHA256 == got.EditedImageSHA256 || got.FirstHistorySHA256 == "" || got.EditedHistorySHA256 == "" || got.Model != options.Model {
		t.Fatal("native image edit evidence was not proven")
	}
}

const hostedNativeReply = "Controlled fixture reply."
const hostedNativeXInput = `{"query":"fixture"}`

type hostedNativeHTTP struct {
	mu        sync.Mutex
	scenario  string
	failure   string
	responses int
	tools     []string
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
		events = append(events, hostedSearchEvents(fixture.scenario)...)
		index = 1
	}
	events = append(events, assistantEvents(index, responseID+"-reply", hostedNativeReply)...)
	events = append(events, map[string]any{"type": "response.completed", "response": map[string]any{"id": responseID}})
	if err := writeSSE(writer, events); err != nil {
		fixture.reject(writer, "write")
	}
}

func (fixture *hostedNativeHTTP) reject(writer http.ResponseWriter, reason string) {
	if fixture.failure == "" {
		fixture.failure = reason
	}
	http.Error(writer, "controlled fixture rejection", http.StatusBadRequest)
}

func hostedToolPolicy(scenario string, tools []json.RawMessage) string {
	var web, x map[string]json.RawMessage
	webCount, xCount := 0, 0
	for _, raw := range tools {
		var kind struct {
			Type string `json:"type"`
		}
		if json.Unmarshal(raw, &kind) != nil {
			return "tool_shape"
		}
		switch kind.Type {
		case "web_search":
			webCount++
			if json.Unmarshal(raw, &web) != nil {
				return "tool_shape"
			}
		case "x_search":
			xCount++
			if json.Unmarshal(raw, &x) != nil {
				return "tool_shape"
			}
		}
	}
	if webCount > 1 || xCount > 1 {
		return "duplicate_hosted_tool"
	}
	switch scenario {
	case "web", "web_excluded":
		if webCount != 1 {
			return "web_search_absent"
		}
	case "x", "x_window":
		if xCount != 1 {
			return "x_search_absent"
		}
	default:
		return "scenario"
	}
	if scenario == "web" {
		if _, ok := web["filters"]; ok {
			return "unexpected_web_filters"
		}
	}
	if scenario == "web_excluded" {
		var filters struct {
			Excluded []string `json:"excluded_domains"`
			Allowed  []string `json:"allowed_domains"`
		}
		raw, ok := web["filters"]
		if !ok || json.Unmarshal(raw, &filters) != nil || len(filters.Allowed) != 0 || len(filters.Excluded) != 1 || filters.Excluded[0] != "example.com" {
			return "excluded_domains"
		}
	}
	if scenario == "x" {
		if _, ok := x["from_date"]; ok {
			return "unexpected_x_dates"
		}
		if _, ok := x["to_date"]; ok {
			return "unexpected_x_dates"
		}
	}
	if scenario == "x_window" {
		var from, to string
		if json.Unmarshal(x["from_date"], &from) != nil || json.Unmarshal(x["to_date"], &to) != nil || from != "2026-08-01" || to != "2026-08-15" {
			return "x_window_dates"
		}
	}
	return ""
}

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

type imageNativeHTTP struct {
	mu                                     sync.Mutex
	model, failure, first, second, toolHit string
	responses, generations, edits          int
	toolNames                              []string
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
	if fixture.responses > 4 {
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

func imageToolName(tools []json.RawMessage) (string, []string) {
	var names []string
	var match string
	for _, raw := range tools {
		var tool struct {
			Type, Name string
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

func functionCallEvents(callID, name, arguments string) []any {
	added := map[string]any{"type": "function_call", "id": callID + "-item", "call_id": callID, "name": name, "arguments": ""}
	done := map[string]any{"type": "function_call", "id": callID + "-item", "call_id": callID, "name": name, "arguments": arguments, "status": "completed"}
	return []any{
		map[string]any{"type": "response.output_item.added", "output_index": 0, "item": added},
		map[string]any{"type": "response.function_call_arguments.delta", "output_index": 0, "delta": arguments},
		map[string]any{"type": "response.output_item.done", "output_index": 0, "item": done},
	}
}

func assistantEvents(index int, id, text string) []any {
	added := map[string]any{"type": "message", "id": id, "role": "assistant", "phase": "final_answer", "content": []any{}}
	done := map[string]any{"type": "message", "id": id, "role": "assistant", "phase": "final_answer", "status": "completed", "content": []any{map[string]any{"type": "output_text", "text": text}}}
	return []any{
		map[string]any{"type": "response.output_item.added", "output_index": index, "item": added},
		map[string]any{"type": "response.output_text.delta", "output_index": index, "delta": text},
		map[string]any{"type": "response.output_item.done", "output_index": index, "item": done},
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

func writeImage(writer http.ResponseWriter, payload string) {
	writer.Header().Set("Content-Type", "application/json")
	fmt.Fprintf(writer, `{"data":[{"b64_json":%q}]}`, payload)
}

func tinyPNG(t *testing.T, pixel color.NRGBA) string {
	t.Helper()
	img := image.NewNRGBA(image.Rect(0, 0, 1, 1))
	img.SetNRGBA(0, 0, pixel)
	var buf bytes.Buffer
	if err := png.Encode(&buf, img); err != nil {
		t.Fatal(err)
	}
	return base64.StdEncoding.EncodeToString(buf.Bytes())
}
