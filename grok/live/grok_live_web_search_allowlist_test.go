package live

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"testing"
)

const webSearchAllowlistDomain = "www.reuters.com"

func TestGrokHostedWebSearchAllowlist(t *testing.T) {
	h := startGrokLive(t, liveOptions{
		disableShell:            true,
		webSearchAllowedDomains: []string{webSearchAllowlistDomain},
	})
	ex := h.acceptedResponses(context.Background())
	if ex == nil || !requestAdvertisesWebSearchAllowedDomain(ex.requestBody, webSearchAllowlistDomain) {
		h.failStage("allowed_domains_advertised", fmt.Sprintf("last /responses advertises web_search filters.allowed_domains containing %s=%t, expected that filter", webSearchAllowlistDomain, ex != nil && requestAdvertisesWebSearchAllowedDomain(ex.requestBody, webSearchAllowlistDomain)))
	}
}

func requestAdvertisesWebSearchAllowedDomain(body []byte, domain string) bool {
	var parsed struct {
		Tools []struct {
			Type    string `json:"type"`
			Filters *struct {
				AllowedDomains []string `json:"allowed_domains"`
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
		for _, got := range tool.Filters.AllowedDomains {
			if got == domain {
				return true
			}
		}
	}
	return false
}

func TestOverlayWebSearchAllowedDomainsWritesConfigTable(t *testing.T) {
	profile, err := os.ReadFile(shippedGrokProfilePath())
	if err != nil {
		t.Fatal(err)
	}
	if bytes.Contains(profile, []byte("[tools.web_search]")) {
		t.Fatal("shipped config.toml.example must not contain [tools.web_search]")
	}
	got := appendConfigTable(profile, "tools.web_search", "allowed_domains = "+tomlStringList([]string{webSearchAllowlistDomain}))
	want := "\n[tools.web_search]\nallowed_domains = [\"" + webSearchAllowlistDomain + "\"]\n"
	if !bytes.Contains(got, []byte(want)) {
		t.Fatal("overlay did not write [tools.web_search] allowed_domains for www.reuters.com")
	}
	if bytes.Contains(profile, []byte("[tools.web_search]")) {
		t.Fatal("overlay mutated the source profile")
	}
}

func TestLastResponsesAdvertisesWebSearchAllowedDomain(t *testing.T) {
	exchanges := []wireExchange{
		{path: "/v1/models", status: 200, requestBody: []byte(`{}`)},
		{path: "/v1/responses", status: 200, requestBody: []byte(`{"tools":[{"type":"web_search"}]}`)},
		{path: "/v1/responses?stream=true", status: 200, requestBody: []byte(`{"tools":[{"type":"function","name":"shell"},{"type":"web_search","filters":{"allowed_domains":["www.reuters.com"]}},{"type":"x_search"}]}`)},
		{path: "/v1/models", status: 200, requestBody: []byte(`{}`)},
	}
	last := lastResponsesExchange(exchanges)
	if last == nil || last.status != 200 || !requestAdvertisesWebSearchAllowedDomain(last.requestBody, webSearchAllowlistDomain) {
		t.Fatalf("last /responses did not advertise www.reuters.com")
	}
	if requestAdvertisesWebSearchAllowedDomain(last.requestBody, "example.com") {
		t.Fatal("unrelated domain must not match")
	}
	if requestAdvertisesWebSearchAllowedDomain(exchanges[1].requestBody, webSearchAllowlistDomain) {
		t.Fatal("bare web_search must not advertise allowed_domains")
	}
}
