//go:build facts

package facts_test

import (
	"context"
	"encoding/json"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/dist"
	"github.com/Harness-X-Harness/codex/grok/facts"
	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/internal/scenarios"
)

// The retained root now uses catalog names: basic_primary/basic_pinned become
// basic-primary/basic-pinned, encrypted_typed_replay becomes encrypted-replay,
// and shipped_routes becomes shipped-routes. No direct backend probe is added.
func TestBackendFacts(t *testing.T) {
	if os.Getenv("GROK_FACTS") != "1" {
		t.Fatal("backend not run: set GROK_FACTS=1 and GROK_API_KEY")
	}
	key := strings.TrimSpace(os.Getenv("GROK_API_KEY"))
	if key == "" {
		t.Fatal("GROK_API_KEY required after backend opt-in")
	}
	probe, err := facts.NewProbe(providerfixture.ResponsesEndpoint, key)
	if err != nil {
		t.Fatal(err)
	}
	selected, err := scenarios.Select("facts", "full", "")
	if err != nil {
		t.Fatal(err)
	}
	for _, scenario := range selected {
		if searchFact(scenario) {
			continue
		}
		t.Run(scenario.Name, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
			defer cancel()
			scenarioProbe := probe
			if scenario.Policy == "shipped-routes" {
				_, baseURL, err := dist.Defaults()
				if err != nil {
					t.Fatal(err)
				}
				scenarioProbe, err = facts.NewProbe(baseURL+"/responses", key)
				if err != nil {
					t.Fatal(err)
				}
			}
			observation, err := scenario.RunFacts(ctx, scenarioProbe)
			encoded, _ := json.Marshal(observation)
			t.Logf("FACT observed_at=%s metadata=%s", time.Now().UTC().Format(time.RFC3339), encoded)
			if err != nil {
				t.Fatalf("NOT_OBSERVED reason=%s", err)
			}
			if !observation.Completed() {
				t.Fatal("NOT_OBSERVED reason=incomplete_backend_fact")
			}
		})
	}
}

func searchFact(scenario scenarios.Scenario) bool {
	return strings.HasPrefix(scenario.Policy, "web") || strings.HasPrefix(scenario.Policy, "x")
}
