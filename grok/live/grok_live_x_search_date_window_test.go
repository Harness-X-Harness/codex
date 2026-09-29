package live

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"testing"
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
	ex := h.acceptedResponses(context.Background())
	if ex == nil || !requestAdvertisesXSearchDateWindow(ex.requestBody, xSearchWindowFromDate, xSearchWindowToDate) {
		h.failStage("x_search_window_advertised", fmt.Sprintf("last /responses advertises x_search from_date=%s to_date=%s=%t, expected that window", xSearchWindowFromDate, xSearchWindowToDate, ex != nil && requestAdvertisesXSearchDateWindow(ex.requestBody, xSearchWindowFromDate, xSearchWindowToDate)))
	}
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
	got := appendConfigTable(profile, "model_providers.grok.x_search",
		`from_date = "`+xSearchWindowFromDate+`"`,
		`to_date = "`+xSearchWindowToDate+`"`,
	)
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
