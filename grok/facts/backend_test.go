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

func TestBackendFacts(t *testing.T) {
	if os.Getenv("GROK_FACTS") != "1" {
		t.Skip("backend not run: set GROK_FACTS=1 and GROK_API_KEY")
	}
	key := strings.TrimSpace(os.Getenv("GROK_API_KEY"))
	if key == "" {
		t.Fatal("GROK_API_KEY required after backend opt-in")
	}
	probe, err := facts.NewProbe(providerfixture.ResponsesEndpoint, key)
	if err != nil {
		t.Fatal(err)
	}
	cases := []struct {
		name, model string
		observe     func(context.Context, string) (facts.Observation, error)
	}{
		{"basic_primary", providerfixture.PrimaryModel, probe.Text},
		{"basic_pinned", providerfixture.PinnedModel, probe.Text},
		{"encrypted_typed_replay", providerfixture.PrimaryModel, probe.EncryptedReplay},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
			defer cancel()
			observation, err := tc.observe(ctx, tc.model)
			t.Logf("FACT observed_at=%s metadata=%+v", time.Now().UTC().Format(time.RFC3339), observation)
			if err != nil {
				t.Fatalf("NOT_OBSERVED reason=%s", err)
			}
		})
	}
}
