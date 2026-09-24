package live

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"testing"
)

const webSearchExcludedDomain = "en.wikipedia.org"

func TestGrokHostedWebSearchExcludedDomains(t *testing.T) {
	h := startGrokLive(t, liveOptions{
		disableShell:             true,
		webSearchExcludedDomains: []string{webSearchExcludedDomain},
	})
	ex := h.acceptedResponses(context.Background())
	if ex == nil || !requestAdvertisesWebSearchExcludedDomain(ex.requestBody, webSearchExcludedDomain) {
		h.failStage("excluded_domains_advertised", fmt.Sprintf("last /responses advertises web_search filters.excluded_domains containing %s=%t, expected that filter", webSearchExcludedDomain, ex != nil && requestAdvertisesWebSearchExcludedDomain(ex.requestBody, webSearchExcludedDomain)))
	}
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
	got := appendConfigTable(profile, "tools.web_search", "excluded_domains = "+tomlStringList([]string{webSearchExcludedDomain}))
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
