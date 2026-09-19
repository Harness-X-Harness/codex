package live

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"testing"
	"time"
)

const webSearchExcludedDomain = "en.wikipedia.org"

func TestGrokHostedWebSearchExcludedDomains(t *testing.T) {
	h := startGrokLive(t, liveOptions{
		disableShell:             true,
		webSearchExcludedDomains: []string{webSearchExcludedDomain},
	})
	ctx := context.Background()
	h.requireGrokCatalog(ctx)

	first := h.runTurn(ctx, startTurnOpts{
		prompt:        "Use web search to find the current top headline from a news publisher (not Wikipedia) and reply with the headline text and its URL. Do not answer from memory and do not use a shell.",
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

	turn1 := lastResponsesExchange(h.recorder.snapshot())
	if turn1 == nil || !requestAdvertisesWebSearchExcludedDomain(turn1.requestBody, webSearchExcludedDomain) {
		present := turn1 != nil
		h.failStage("excluded_domains_advertised", fmt.Sprintf("Turn 1 last /responses present=%t advertises web_search filters.excluded_domains containing %s=%t, expected that filter", present, webSearchExcludedDomain, present && requestAdvertisesWebSearchExcludedDomain(turn1.requestBody, webSearchExcludedDomain)))
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

func overlayWebSearchExcludedDomains(config []byte, domains []string) []byte {
	if len(domains) == 0 {
		return config
	}
	quoted := make([]string, 0, len(domains))
	for _, domain := range domains {
		quoted = append(quoted, `"`+domain+`"`)
	}
	extra := []byte("\n[tools.web_search]\nexcluded_domains = [" + strings.Join(quoted, ", ") + "]\n")
	out := make([]byte, len(config)+len(extra))
	copy(out, config)
	copy(out[len(config):], extra)
	return out
}

func requestAdvertisesWebSearchExcludedDomain(body []byte, domain string) bool {
	var parsed struct {
		Tools []struct {
			Type    string `json:"type"`
			Filters *struct {
				ExcludedDomains []string `json:"excluded_domains"`
			} `json:"filters"`
		} `json:"tools"`
	}
	if json.Unmarshal(body, &parsed) != nil {
		return false
	}
	for _, tool := range parsed.Tools {
		if tool.Type != "web_search" || tool.Filters == nil {
			continue
		}
		for _, got := range tool.Filters.ExcludedDomains {
			if got == domain {
				return true
			}
		}
	}
	return false
}

func TestOverlayWebSearchExcludedDomainsWritesConfigTable(t *testing.T) {
	profile, err := os.ReadFile(shippedGrokProfilePath())
	if err != nil {
		t.Fatal(err)
	}
	want := "\n[tools.web_search]\nexcluded_domains = [\"" + webSearchExcludedDomain + "\"]\n"
	if bytes.Contains(profile, []byte("[tools.web_search]")) {
		t.Fatal("shipped config.toml.example must not contain [tools.web_search]")
	}
	if bytes.Contains(profile, []byte("excluded_domains")) || bytes.Contains(profile, []byte(want)) {
		t.Fatal("shipped config.toml.example must not contain [tools.web_search] excluded_domains overlay")
	}
	got := overlayWebSearchExcludedDomains(profile, []string{webSearchExcludedDomain})
	if !bytes.Contains(got, []byte(want)) {
		t.Fatal("overlay did not write [tools.web_search] excluded_domains for en.wikipedia.org")
	}
	if bytes.Contains(got, []byte("allowed_domains")) {
		t.Fatal("excluded_domains overlay must not write allowed_domains")
	}
	if bytes.Contains(profile, []byte("[tools.web_search]")) || bytes.Contains(profile, []byte(want)) {
		t.Fatal("overlay mutated the source profile")
	}
}

func TestLastResponsesAdvertisesWebSearchExcludedDomain(t *testing.T) {
	exchanges := []wireExchange{
		{path: "/v1/models", status: 200, requestBody: []byte(`{}`)},
		{path: "/v1/responses", status: 200, requestBody: []byte(`{"tools":[{"type":"web_search"}]}`)},
		{path: "/v1/responses?stream=true", status: 200, requestBody: []byte(`{"tools":[{"type":"function","name":"shell"},{"type":"web_search","filters":{"excluded_domains":["en.wikipedia.org"]}},{"type":"x_search"}]}`)},
		{path: "/v1/models", status: 200, requestBody: []byte(`{}`)},
	}
	last := lastResponsesExchange(exchanges)
	if last == nil || last.status != 200 || !requestAdvertisesWebSearchExcludedDomain(last.requestBody, webSearchExcludedDomain) {
		t.Fatalf("last /responses did not advertise en.wikipedia.org")
	}
	if requestAdvertisesWebSearchExcludedDomain(last.requestBody, "example.com") {
		t.Fatal("unrelated domain must not match")
	}
	if requestAdvertisesWebSearchExcludedDomain(exchanges[1].requestBody, webSearchExcludedDomain) {
		t.Fatal("bare web_search must not advertise excluded_domains")
	}
}
