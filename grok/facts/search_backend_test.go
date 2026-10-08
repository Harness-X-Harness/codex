package facts_test

import (
	"context"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/facts"
	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
)

func TestBackendSearchFacts(t *testing.T) {
	if os.Getenv("GROK_FACTS") != "1" {
		t.Skip("backend not run: explicit Facts opt-in required")
	}
	key := strings.TrimSpace(os.Getenv("GROK_API_KEY"))
	if key == "" {
		t.Fatal("GROK_API_KEY required after backend opt-in")
	}
	probe, err := facts.NewProbe(providerfixture.ResponsesEndpoint, key)
	if err != nil {
		t.Fatal(err)
	}
	for _, scenario := range []string{"web", "web_allowed", "web_excluded", "x", "x_window"} {
		t.Run(scenario, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
			defer cancel()
			observed, err := probe.SearchPolicy(ctx, providerfixture.PrimaryModel, scenario)
			t.Logf("SEARCH_POLICY observed_at=%s metadata=%+v", time.Now().UTC().Format(time.RFC3339), observed)
			if err != nil {
				t.Fatalf("NOT_OBSERVED reason=%s", err)
			}
		})
	}
	for _, scenario := range []string{"web", "x"} {
		t.Run(scenario+"_replay", func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
			defer cancel()
			observed, err := probe.SearchReplay(ctx, providerfixture.PrimaryModel, scenario)
			t.Logf("SEARCH_REPLAY observed_at=%s metadata=%+v", time.Now().UTC().Format(time.RFC3339), observed)
			if err != nil {
				t.Fatalf("NOT_OBSERVED reason=%s", err)
			}
		})
	}
}
