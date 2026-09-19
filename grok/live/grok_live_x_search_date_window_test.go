package live

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"testing"
	"time"
)

const (
	xSearchWindowFromDate = "2026-08-01"
	xSearchWindowToDate   = "2026-09-16"
)

func TestGrokHostedXSearchDateWindow(t *testing.T) {
	h := startGrokLive(t, liveOptions{
		disableShell: true,
		xSearchWindow: xSearchDateWindow{
			fromDate: xSearchWindowFromDate,
			toDate:   xSearchWindowToDate,
		},
	})
	ctx := context.Background()
	h.requireGrokCatalog(ctx)

	first := h.runTurn(ctx, startTurnOpts{
		prompt:        "Use X search once to find one recent post from the @xai account on X. Reply with its date and the first sentence. Do not run extra searches, do not use web search, do not answer from memory, and do not use a shell.",
		deadline:      5 * time.Minute,
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

	turn1 := lastResponsesExchange(h.recorder.snapshot())
	if turn1 == nil || !requestAdvertisesXSearchDateWindow(turn1.requestBody, xSearchWindowFromDate, xSearchWindowToDate) {
		present := turn1 != nil
		h.failStage("x_search_window_advertised", fmt.Sprintf("Turn 1 last /responses present=%t advertises x_search from_date=%s to_date=%s=%t, expected that window", present, xSearchWindowFromDate, xSearchWindowToDate, present && requestAdvertisesXSearchDateWindow(turn1.requestBody, xSearchWindowFromDate, xSearchWindowToDate)))
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
	replayed := false
	if ex != nil {
		for _, name := range hostedNames(scan) {
			if requestReplaysHostedCustomToolCall(ex.requestBody, name) {
				replayed = true
				break
			}
		}
	}
	if ex == nil || ex.status < 200 || ex.status > 299 || !replayed {
		status := 0
		if ex != nil {
			status = ex.status
		}
		h.failStage("hosted_call_replayed_accepted", fmt.Sprintf("last /responses present=%t status=%d replayed_hosted_custom_tool_call=%t names=%v, expected 2xx with a replayed custom_tool_call in %v", ex != nil, status, replayed, hostedNames(scan), hostedXSearchNames))
	}
}

func overlayXSearchDateWindow(config []byte, window xSearchDateWindow) []byte {
	if window.fromDate == "" || window.toDate == "" {
		return config
	}
	extra := []byte("\n[model_providers.grok.x_search]\nfrom_date = \"" + window.fromDate + "\"\nto_date = \"" + window.toDate + "\"\n")
	out := make([]byte, len(config)+len(extra))
	copy(out, config)
	copy(out[len(config):], extra)
	return out
}

func requestAdvertisesXSearchDateWindow(body []byte, fromDate, toDate string) bool {
	var parsed struct {
		Tools []struct {
			Type     string `json:"type"`
			FromDate string `json:"from_date"`
			ToDate   string `json:"to_date"`
		} `json:"tools"`
	}
	if json.Unmarshal(body, &parsed) != nil {
		return false
	}
	for _, tool := range parsed.Tools {
		if tool.Type == "x_search" && tool.FromDate == fromDate && tool.ToDate == toDate {
			return true
		}
	}
	return false
}

func TestOverlayXSearchDateWindowWritesConfigTable(t *testing.T) {
	profile, err := os.ReadFile(shippedGrokProfilePath())
	if err != nil {
		t.Fatal(err)
	}
	if bytes.Contains(profile, []byte("[model_providers.grok.x_search]")) {
		t.Fatal("shipped config.toml.example must not contain [model_providers.grok.x_search]")
	}
	got := overlayXSearchDateWindow(profile, xSearchDateWindow{
		fromDate: xSearchWindowFromDate,
		toDate:   xSearchWindowToDate,
	})
	want := "\n[model_providers.grok.x_search]\nfrom_date = \"" + xSearchWindowFromDate + "\"\nto_date = \"" + xSearchWindowToDate + "\"\n"
	if !bytes.Contains(got, []byte(want)) {
		t.Fatal("overlay did not write [model_providers.grok.x_search] from_date/to_date")
	}
	if bytes.Contains(profile, []byte("[model_providers.grok.x_search]")) {
		t.Fatal("overlay mutated the source profile")
	}
}

func TestLastResponsesAdvertisesXSearchDateWindow(t *testing.T) {
	exchanges := []wireExchange{
		{path: "/v1/models", status: 200, requestBody: []byte(`{}`)},
		{path: "/v1/responses", status: 200, requestBody: []byte(`{"tools":[{"type":"x_search"}]}`)},
		{path: "/v1/responses?stream=true", status: 200, requestBody: []byte(`{"tools":[{"type":"function","name":"shell"},{"type":"web_search"},{"type":"x_search","from_date":"2026-08-01","to_date":"2026-09-16"}]}`)},
		{path: "/v1/models", status: 200, requestBody: []byte(`{}`)},
	}
	last := lastResponsesExchange(exchanges)
	if last == nil || last.status != 200 || !requestAdvertisesXSearchDateWindow(last.requestBody, xSearchWindowFromDate, xSearchWindowToDate) {
		t.Fatalf("last /responses did not advertise x_search date window")
	}
	if requestAdvertisesXSearchDateWindow(last.requestBody, "2026-01-01", "2026-01-31") {
		t.Fatal("mismatched dates must not match")
	}
	if requestAdvertisesXSearchDateWindow(exchanges[1].requestBody, xSearchWindowFromDate, xSearchWindowToDate) {
		t.Fatal("bare x_search must not advertise a date window")
	}
}
