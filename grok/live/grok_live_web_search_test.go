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
)

func TestGrokHostedWebSearch(t *testing.T) {
	h := startGrokLive(t, liveOptions{disableShell: true})
	ctx := context.Background()
	h.requireGrokCatalog(ctx)

	first := h.runTurn(ctx, startTurnOpts{
		prompt:        "Use web search to find the current top headline on https://www.reuters.com and reply with the headline text and its URL. Do not answer from memory and do not use a shell.",
		deadline:      3 * time.Minute,
		approvalNever: true,
		disableShell:  true,
	})
	if first.Provider != grokProvider {
		h.failStage("thread_bound_to_grok", fmt.Sprintf("Thread Provider is %q, expected %q", first.Provider, grokProvider))
	}
	if !first.completed() || first.reply() == "" {
		h.failStage("turn_completed", fmt.Sprintf("Turn 1 status is %s with agent_message=%t deadline=%t, expected terminal completed with an agent message", first.Status, first.reply() != "", first.DeadlineHit))
	}
	if itemID, streamed, completed, lost := streamedTextLoss(first.Result); lost {
		h.failStage("streamed_text_matches_completed", fmt.Sprintf("agent message %s streamed %d chars but completed with %d, expected every delta to reach the client (Grok interleaves hosted items with the open message; see #247)", h.redact(itemID), streamed, completed))
	}

	var items []durableResponseItem
	if !waitDurable(rolloutSettle, func() bool {
		items = scanDurableResponseItems(h.home)
		for _, item := range items {
			if item.Type == "web_search_call" {
				return true
			}
		}
		return false
	}) {
		h.failStage("hosted_web_search_call_recorded", fmt.Sprintf("session JSONL response_item types are [%s], expected web_search_call", durableItemSummary(items)))
	}

	second := h.runTurn(ctx, startTurnOpts{
		threadID: first.ThreadID,
		prompt:   "Which site did that headline come from? Reply in one line.",
		deadline: 2 * time.Minute,
	})
	if second.ThreadID != first.ThreadID || !second.completed() {
		h.failStage("follow_up_turn_completed", fmt.Sprintf("Turn 2 same_thread=%t status=%s deadline=%t, expected terminal completed on the same Thread", second.ThreadID == first.ThreadID, second.Status, second.DeadlineHit))
	}

	last := lastResponsesExchange(h.recorder.snapshot())
	if last == nil {
		h.failStage("web_search_call_replayed_accepted", "no /responses exchange recorded, expected a replayed web_search_call with 2xx")
		return
	}
	call := firstInputItemByType(last.requestBody, "web_search_call")
	if call == nil {
		h.failStage("web_search_call_replayed_accepted", fmt.Sprintf("last /responses request has no web_search_call input item (HTTP %d), expected a replayed web_search_call with 2xx", last.status))
		return
	}
	if last.status < 200 || last.status > 299 {
		h.failStage("web_search_call_replayed_accepted", fmt.Sprintf("replayed web_search_call (%s) returned HTTP %d (%s), expected 2xx", hostedCallKeyPresence(call), last.status, collapsedBackendError(string(last.responseBody))))
	}
}

type durableResponseItem struct {
	Type string
	Name string
}

func scanDurableResponseItems(home string) []durableResponseItem {
	var items []durableResponseItem
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
			if json.Unmarshal(raw, &rec) != nil || rec.Type != "response_item" {
				continue
			}
			var obj map[string]json.RawMessage
			if json.Unmarshal(rec.Payload, &obj) != nil {
				continue
			}
			var item durableResponseItem
			_ = json.Unmarshal(obj["type"], &item.Type)
			_ = json.Unmarshal(obj["name"], &item.Name)
			if item.Type == "" {
				continue
			}
			items = append(items, item)
		}
		return nil
	})
	return items
}

func durableItemSummary(items []durableResponseItem) string {
	if len(items) == 0 {
		return "none"
	}
	type key struct{ typ, name string }
	counts := make(map[key]int)
	var order []key
	for _, item := range items {
		k := key{item.Type, item.Name}
		if _, seen := counts[k]; !seen {
			order = append(order, k)
		}
		counts[k]++
	}
	parts := make([]string, 0, len(order))
	for _, k := range order {
		label := k.typ
		if k.name != "" {
			label += " name=" + k.name
		}
		parts = append(parts, fmt.Sprintf("%s=%d", label, counts[k]))
	}
	return strings.Join(parts, ", ")
}

func lastResponsesExchange(exchanges []wireExchange) *wireExchange {
	var last *wireExchange
	for i := range exchanges {
		path := exchanges[i].path
		if q := strings.IndexByte(path, '?'); q >= 0 {
			path = path[:q]
		}
		if strings.HasSuffix(path, "/responses") {
			last = &exchanges[i]
		}
	}
	return last
}

func firstInputItemByType(body []byte, typ string) map[string]any {
	var parsed struct {
		Input []json.RawMessage `json:"input"`
	}
	if json.Unmarshal(body, &parsed) != nil {
		return nil
	}
	for _, raw := range parsed.Input {
		var item map[string]any
		if json.Unmarshal(raw, &item) != nil {
			continue
		}
		if got, _ := item["type"].(string); got == typ {
			return item
		}
	}
	return nil
}

func hostedCallKeyPresence(item map[string]any) string {
	parts := make([]string, 0, 3)
	for _, key := range []string{"id", "action", "status"} {
		if _, ok := item[key]; ok {
			parts = append(parts, key+" present")
		} else {
			parts = append(parts, key+" absent")
		}
	}
	return strings.Join(parts, ", ")
}

func collapsedBackendError(text string) string {
	text = strings.ReplaceAll(text, "\r\n", " ")
	text = strings.ReplaceAll(text, "\n", " ")
	text = strings.ReplaceAll(text, "\r", " ")
	text = strings.Join(strings.Fields(text), " ")
	runes := []rune(text)
	if len(runes) > 160 {
		text = string(runes[:160])
	}
	if text == "" {
		return "empty"
	}
	return text
}

func TestScanDurableResponseItemsRecordsTypeAndName(t *testing.T) {
	home := t.TempDir()
	dir := filepath.Join(home, "sessions")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	lines := strings.Join([]string{
		`{"type":"event_msg","payload":{"type":"item_completed"}}`,
		`{"type":"response_item","payload":{"type":"message","role":"user"}}`,
		`{"type":"response_item","payload":{"type":"custom_tool_call","name":"x_keyword_search"}}`,
		`{"type":"response_item","payload":{"type":"web_search_call","id":"ws_1","action":{"type":"search"}}}`,
	}, "\n") + "\n"
	if err := os.WriteFile(filepath.Join(dir, "rollout.jsonl"), []byte(lines), 0o600); err != nil {
		t.Fatal(err)
	}
	got := scanDurableResponseItems(home)
	want := []durableResponseItem{
		{Type: "message"},
		{Type: "custom_tool_call", Name: "x_keyword_search"},
		{Type: "web_search_call"},
	}
	if fmt.Sprintf("%v", got) != fmt.Sprintf("%v", want) {
		t.Fatalf("items = %#v, want %#v", got, want)
	}
	if summary := durableItemSummary(got); summary != "message=1, custom_tool_call name=x_keyword_search=1, web_search_call=1" {
		t.Fatalf("summary = %q", summary)
	}
}

func TestLastResponsesWebSearchCallKeyPresence(t *testing.T) {
	exchanges := []wireExchange{
		{path: "/v1/models", status: 200, requestBody: []byte(`{}`)},
		{path: "/v1/responses", status: 200, requestBody: []byte(`{"input":[{"type":"message"}]}`)},
		{path: "/v1/responses?stream=true", status: 422, requestBody: []byte(`{"input":[{"type":"web_search_call","id":"ws_1","action":{"type":"search"}}]}`), responseBody: []byte("missing field action")},
		{path: "/v1/models", status: 200, requestBody: []byte(`{}`)},
	}
	last := lastResponsesExchange(exchanges)
	if last == nil || last.status != 422 {
		t.Fatalf("last = %#v", last)
	}
	call := firstInputItemByType(last.requestBody, "web_search_call")
	if got := hostedCallKeyPresence(call); got != "id present, action present, status absent" {
		t.Fatalf("keys = %q", got)
	}
}
