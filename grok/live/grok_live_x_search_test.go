package live

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// hostedXSearchNames mirrors GrokModelProvider::is_provider_hosted_tool_call
// in codex-rs/model-provider/src/grok_provider.rs. The Rust predicate is the
// authority.
var hostedXSearchNames = []string{
	"x_keyword_search",
	"x_semantic_search",
	"x_user_search",
	"x_thread_fetch",
}

func TestGrokHostedXSearch(t *testing.T) {
	h := startGrokLive(t, liveOptions{disableShell: true})
	ctx := context.Background()
	h.requireGrokCatalog(ctx)

	first := h.runTurn(ctx, startTurnOpts{
		prompt:        "Use X search to find the most recent post from the @xai account on X and reply with its date and the first sentence. Do not answer from memory and do not use a shell.",
		deadline:      3 * time.Minute,
		approvalNever: true,
		disableShell:  true,
	})
	if first.Provider != grokProvider {
		h.failStage("thread_bound_to_grok", fmt.Sprintf("Thread Provider is %q, expected %q", first.Provider, grokProvider))
	}
	if !first.completed() || first.reply() == "" {
		h.failStage("turn_completed", fmt.Sprintf("Turn 1 status=%q agent_message=%t, expected completed with an agent message", first.Status, first.reply() != ""))
	}
	if itemID, streamed, completed, lost := streamedTextLoss(first.Result); lost {
		h.failStage("streamed_text_matches_completed", fmt.Sprintf("agent message %s streamed %d chars but completed with %d, expected every delta to reach the client (Grok interleaves the hosted call with the open message; see #247)", h.redact(itemID), streamed, completed))
	}

	var scan xSearchDurable
	waitDurable(rolloutSettle, func() bool {
		scan = scanXSearchDurable(h.home)
		return completedHostedXSearch(scan) != nil
	})
	hosted := completedHostedXSearch(scan)
	if hosted == nil {
		h.failStage("hosted_x_search_call_recorded", fmt.Sprintf("session completed hosted custom_tool_call count=0 names=%v statuses=%v, expected one completed call in %v", hostedNames(scan), hostedStatuses(scan), hostedXSearchNames))
	}
	if dispatched, how := clientDispatchFor(scan, first.Items, *hosted); dispatched {
		h.failStage("client_dispatch_absent", fmt.Sprintf("observed %s, expected the hosted call to be recorded without client dispatch", how))
	}

	second := h.runTurn(ctx, startTurnOpts{
		threadID: first.ThreadID,
		prompt:   "Which X handle did that post come from? Reply in one line.",
		deadline: 2 * time.Minute,
	})
	if second.ThreadID != first.ThreadID || !second.completed() {
		sameThread := second.ThreadID == first.ThreadID
		h.failStage("follow_up_turn_completed", fmt.Sprintf("Turn 2 status=%q same_thread=%t, expected completed on the same Thread", second.Status, sameThread))
	}

	ex := lastResponsesExchange(h.recorder.snapshot())
	replayed := ex != nil && requestReplaysHostedCustomToolCall(ex.requestBody, hosted.Name)
	if ex == nil || ex.status < 200 || ex.status > 299 || !replayed {
		status := 0
		if ex != nil {
			status = ex.status
		}
		h.failStage("hosted_call_replayed_accepted", fmt.Sprintf("last /responses present=%t status=%d replayed_hosted_custom_tool_call=%t, expected 2xx with type custom_tool_call name %s", ex != nil, status, replayed, hosted.Name))
	}
}

type hostedXSearchCall struct {
	Name   string
	CallID string
	Status string
}

type xSearchDurable struct {
	hosted        []hostedXSearchCall
	outputCallIDs map[string]struct{}
	dynamicNames  map[string]struct{}
	dynamicIDs    map[string]struct{}
}

func scanXSearchDurable(home string) xSearchDurable {
	scan := xSearchDurable{
		outputCallIDs: map[string]struct{}{},
		dynamicNames:  map[string]struct{}{},
		dynamicIDs:    map[string]struct{}{},
	}
	hosted := make(map[string]struct{}, len(hostedXSearchNames))
	for _, name := range hostedXSearchNames {
		hosted[name] = struct{}{}
	}
	_ = filepath.WalkDir(filepath.Join(home, "sessions"), func(path string, entry fs.DirEntry, err error) error {
		if err != nil || entry.IsDir() || !strings.HasSuffix(path, ".jsonl") {
			return err
		}
		data, readErr := os.ReadFile(path)
		if readErr != nil {
			return readErr
		}
		for _, raw := range bytes.Split(data, []byte("\n")) {
			raw = bytes.TrimSpace(raw)
			if len(raw) == 0 {
				continue
			}
			var rec struct {
				Type    string          `json:"type"`
				Payload json.RawMessage `json:"payload"`
			}
			if json.Unmarshal(raw, &rec) != nil {
				continue
			}
			switch rec.Type {
			case "response_item":
				scanXSearchResponseItem(&scan, hosted, rec.Payload)
			case "event_msg":
				scanXSearchEventMsg(&scan, rec.Payload)
			}
		}
		return nil
	})
	return scan
}

func scanXSearchResponseItem(scan *xSearchDurable, hosted map[string]struct{}, payload json.RawMessage) {
	var item struct {
		Type   string `json:"type"`
		Name   string `json:"name"`
		CallID string `json:"call_id"`
		Status string `json:"status"`
	}
	if json.Unmarshal(payload, &item) != nil {
		return
	}
	switch item.Type {
	case "custom_tool_call":
		if _, ok := hosted[item.Name]; ok {
			scan.hosted = append(scan.hosted, hostedXSearchCall{
				Name:   item.Name,
				CallID: item.CallID,
				Status: item.Status,
			})
		}
	case "custom_tool_call_output", "function_call_output":
		if item.CallID != "" {
			scan.outputCallIDs[item.CallID] = struct{}{}
		}
	}
}

func scanXSearchEventMsg(scan *xSearchDurable, payload json.RawMessage) {
	var event struct {
		Type string          `json:"type"`
		Item json.RawMessage `json:"item"`
	}
	if json.Unmarshal(payload, &event) != nil || event.Type != "item_completed" || len(event.Item) == 0 {
		return
	}
	var obj map[string]json.RawMessage
	if json.Unmarshal(event.Item, &obj) != nil {
		return
	}
	kind := jsonString(obj, "kind", "type")
	if !isDynamicToolCallKind(kind) {
		return
	}
	if name := jsonString(obj, "tool", "name"); name != "" {
		scan.dynamicNames[name] = struct{}{}
	}
	if id := jsonString(obj, "call_id", "callId", "id"); id != "" {
		scan.dynamicIDs[id] = struct{}{}
	}
}

func jsonString(obj map[string]json.RawMessage, keys ...string) string {
	for _, key := range keys {
		raw, ok := obj[key]
		if !ok {
			continue
		}
		var value string
		if json.Unmarshal(raw, &value) == nil {
			return value
		}
	}
	return ""
}

func isDynamicToolCallKind(kind string) bool {
	switch kind {
	case "dynamic_tool_call", "dynamicToolCall", "DynamicToolCall":
		return true
	default:
		return false
	}
}

func completedHostedXSearch(scan xSearchDurable) *hostedXSearchCall {
	for i := range scan.hosted {
		call := &scan.hosted[i]
		if call.Status == "completed" {
			return call
		}
	}
	return nil
}

func hostedNames(scan xSearchDurable) []string {
	names := make([]string, 0, len(scan.hosted))
	for _, call := range scan.hosted {
		names = append(names, call.Name)
	}
	return names
}

func hostedStatuses(scan xSearchDurable) []string {
	statuses := make([]string, 0, len(scan.hosted))
	for _, call := range scan.hosted {
		statuses = append(statuses, call.Status)
	}
	return statuses
}

func clientDispatchFor(scan xSearchDurable, items []protocolv2.ThreadItem, call hostedXSearchCall) (bool, string) {
	if _, ok := scan.outputCallIDs[call.CallID]; ok {
		return true, "session custom_tool_call_output or function_call_output for the hosted call_id"
	}
	if _, ok := scan.dynamicNames[call.Name]; ok {
		return true, "session dynamic_tool_call-style item named as the hosted call"
	}
	if _, ok := scan.dynamicIDs[call.CallID]; ok {
		return true, "session dynamic_tool_call-style item for the hosted call_id"
	}
	for _, item := range items {
		if dyn, ok := item.AsDynamicToolCall(); ok && (dyn.Tool == call.Name || dyn.ID == call.CallID) {
			return true, "Turn items include a dynamic_tool_call for the hosted call"
		}
		if out, ok := item.AsFunctionCallOutput(); ok && (out.Name == call.Name || out.ID == call.CallID) {
			return true, "Turn items include a functionCallOutput for the hosted call"
		}
	}
	return false, ""
}

func requestReplaysHostedCustomToolCall(body []byte, name string) bool {
	var req struct {
		Input []struct {
			Type string `json:"type"`
			Name string `json:"name"`
		} `json:"input"`
	}
	if json.Unmarshal(body, &req) != nil {
		return false
	}
	for _, item := range req.Input {
		if item.Type == "custom_tool_call" && item.Name == name {
			return true
		}
	}
	return false
}

func TestScanXSearchDurableFindsHostedCallAndClientOutputs(t *testing.T) {
	home := t.TempDir()
	dir := filepath.Join(home, "sessions")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	prompt := "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"x_keyword_search\"}]}}\n"
	hosted := "{\"type\":\"response_item\",\"payload\":{\"type\":\"custom_tool_call\",\"name\":\"x_keyword_search\",\"call_id\":\"c1\",\"status\":\"completed\"}}\n"
	output := "{\"type\":\"response_item\",\"payload\":{\"type\":\"custom_tool_call_output\",\"call_id\":\"c1\"}}\n"
	dyn := "{\"type\":\"event_msg\",\"payload\":{\"type\":\"item_completed\",\"item\":{\"type\":\"dynamicToolCall\",\"tool\":\"x_keyword_search\",\"id\":\"c1\"}}}\n"
	if err := os.WriteFile(filepath.Join(dir, "rollout.jsonl"), []byte(prompt+hosted+output+dyn), 0o600); err != nil {
		t.Fatal(err)
	}
	scan := scanXSearchDurable(home)
	got := completedHostedXSearch(scan)
	want := &hostedXSearchCall{Name: "x_keyword_search", CallID: "c1", Status: "completed"}
	if got == nil || *got != *want {
		t.Fatalf("hosted call = %+v, want %+v", got, want)
	}
	if _, ok := scan.outputCallIDs["c1"]; !ok {
		t.Fatal("expected output call_id c1")
	}
	if _, ok := scan.dynamicNames["x_keyword_search"]; !ok {
		t.Fatal("expected dynamic tool name")
	}
	if dispatched, _ := clientDispatchFor(scan, nil, *got); !dispatched {
		t.Fatal("expected client dispatch for output and dynamic item")
	}
	clean := scanXSearchDurable(home)
	clean.outputCallIDs = map[string]struct{}{}
	clean.dynamicNames = map[string]struct{}{}
	clean.dynamicIDs = map[string]struct{}{}
	if dispatched, how := clientDispatchFor(clean, nil, *got); dispatched {
		t.Fatalf("prompt text must not prove dispatch: %s", how)
	}
}

func TestRequestReplaysHostedCustomToolCall(t *testing.T) {
	body := []byte(`{"input":[{"type":"message","role":"user"},{"type":"custom_tool_call","name":"x_user_search","call_id":"c1"}]}`)
	if !requestReplaysHostedCustomToolCall(body, "x_user_search") {
		t.Fatal("expected hosted custom_tool_call replay")
	}
	if requestReplaysHostedCustomToolCall(body, "x_keyword_search") {
		t.Fatal("unknown hosted name must not match")
	}
	exchanges := []wireExchange{
		{path: "/v1/models", status: 200},
		{path: "/v1/responses", status: 200, requestBody: []byte(`{"input":[]}`)},
		{path: "/v1/responses?beta=1", status: 200, requestBody: body},
	}
	last := lastResponsesExchange(exchanges)
	if last == nil || last.status != 200 || !requestReplaysHostedCustomToolCall(last.requestBody, "x_user_search") {
		t.Fatalf("last /responses = %#v", last)
	}
}
