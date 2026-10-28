package live_test

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/live"
)

// These are controlled protocol fixtures, not paid backend requests. The output
// artifact, rather than a passing oracle alone, must explain each outcome.
func TestProductDiagnosticsRetainSuccessfulAndRejectedObservations(t *testing.T) {
	tests := []struct {
		name, fixture, scenario string
		want                    []string
		fails                   bool
	}{
		{"catalog", "shipped:ok", "catalog", []string{"catalog_result", "actual", "expected", "grok-4.7"}, false},
		{"catalog_mismatch", "shipped:missing_model", "catalog", []string{"catalog_result", "actual", "expected", "matches"}, true},
		{"child", "shipped:ok", "child", []string{"child_setup", "child_candidate", "child_result_use", "child_visible_message", "product_turn_request"}, false},
		{"child_wrong_marker", "shipped:wrong_marker", "child", []string{"child_reply_check", "marker_recalled", "child_result"}, true},
		{"child_wrong_binding", "shipped:child_parent", "child", []string{"child_candidate", "eligible", "PRIVATE_OTHER"}, true},
		{"web", "search:ok:web", "web", []string{"search_request", "search_answer", "PRIVATE_HEADLINE", "PRIVATE_QUERY", "search_followup_request"}, false},
		{"web_allowed", "search:ok:web_allowed", "web_allowed", []string{"allowed_domains", "reuters.com", "search_answer"}, false},
		{"web_excluded", "search:ok:web_excluded", "web_excluded", []string{"excluded_domains", "example.com", "search_answer"}, false},
		{"web_wrong_read", "search:wrong_read:web", "web", []string{"search_read_snapshot", "expected_thread_id", "search_read_turn", "PRIVATE_HEADLINE"}, true},
		{"web_prior_failed", "search:prior_failed:web", "web", []string{"search_read_turn", "failed", "PRIVATE_HEADLINE"}, true},
		{"web_failed_initial", "search:failed_initial:web", "web", []string{"search_start_snapshot", "failed", "error"}, true},
		{"web_missing_result", "search:missing:web", "web", []string{"search_request", "search_settled", "PRIVATE_HEADLINE"}, true},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			options := fixtureOptions(t, test.fixture)
			dir := t.TempDir()
			diagnostics := live.NewDiagnostics(dir, options.APIKey)
			options.Diagnostics = diagnostics
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			var err error
			switch test.scenario {
			case "catalog":
				_, err = live.ShippedCatalog(ctx, options.Subject, options.APIKey, diagnostics)
			case "child":
				_, err = live.ShippedChildCollaboration(ctx, options.Subject, options.APIKey, diagnostics)
			default:
				_, err = live.HostedSearch(ctx, options, test.scenario)
			}
			diagnostics.Close()
			if (err != nil) != test.fails {
				t.Fatalf("unexpected oracle result: %v", err)
			}
			var contents strings.Builder
			if err := filepath.WalkDir(dir, func(path string, entry os.DirEntry, err error) error {
				if err != nil {
					return err
				}
				if entry.IsDir() {
					return nil
				}
				data, err := os.ReadFile(path)
				contents.Write(data)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			for _, want := range test.want {
				if !strings.Contains(contents.String(), want) {
					t.Errorf("retained diagnostic missing %q", want)
				}
			}
			if strings.Contains(contents.String(), options.APIKey) {
				t.Error("credential escaped")
			}
		})
	}
}
