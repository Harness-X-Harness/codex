//go:build facts

package facts_test

import (
	"context"
	"encoding/json"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/facts"
	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/internal/scenarios"
)

// Existing Web/X subtest names change underscores to the catalog's hyphens,
// including web_replay/x_replay. Policy metadata owns search grouping.
func TestBackendSearchFacts(t *testing.T) {
	if os.Getenv("GROK_FACTS") != "1" {
		t.Fatal("backend not run: explicit Facts opt-in required")
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
		if !searchFact(scenario) {
			continue
		}
		t.Run(scenario.Name, func(t *testing.T) {
			deadline := 2 * time.Minute
			if scenario.SemanticTurns > 1 {
				deadline = 3 * time.Minute
			}
			ctx, cancel := context.WithTimeout(context.Background(), deadline)
			defer cancel()
			observation, err := scenario.RunFacts(ctx, probe)
			encoded, _ := json.Marshal(observation)
			t.Logf("SEARCH_FACT observed_at=%s metadata=%s", time.Now().UTC().Format(time.RFC3339), encoded)
			if err != nil {
				t.Fatalf("NOT_OBSERVED reason=%s", err)
			}
			if !observation.Completed() {
				t.Fatal("NOT_OBSERVED reason=incomplete_backend_fact")
			}
		})
	}
}
