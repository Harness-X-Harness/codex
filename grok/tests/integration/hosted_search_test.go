//go:build integration

package integration_test

import (
	"context"
	"encoding/json"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

// Controlled Web completions prove the supported product composition. Retired
// X cases are explicit negatives and cannot be credited as positive search.
func TestHostedSearchFixture(t *testing.T) {
	subject := nativeSubject(t)
	for _, scenario := range []string{"web", "web_allowed", "web_excluded", "x_retired", "x_window_retired", "unexpected_x_rejected"} {
		t.Run(scenario, func(t *testing.T) {
			integrationObservation(t)
			selected := scenario
			fixture := &hostedNativeHTTP{scenario: scenario}
			if strings.HasSuffix(scenario, "_retired") {
				selected = strings.TrimSuffix(scenario, "_retired")
			}
			if scenario == "unexpected_x_rejected" {
				selected = "web"
				fixture.scenario = "web"
				fixture.unexpectedX = true
			}
			backend := httptest.NewServer(fixture)
			defer backend.Close()
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
			defer cancel()
			options := live.Options{Subject: subject, Model: providerfixture.PrimaryModel, BaseURL: backend.URL + "/v1", APIKey: "fixture-key"}
			got, err := live.HostedSearch(ctx, options, selected)
			fixture.mu.Lock()
			defer fixture.mu.Unlock()
			encoded, _ := json.Marshal(got)
			t.Logf("controlled %s: evidence=%s responses=%d failure=%s tools=%s", scenario, encoded, fixture.responses, fixture.failure, strings.Join(fixture.tools, " | "))
			if strings.HasSuffix(scenario, "_retired") {
				if err == nil || live.DescribeFailure(err).Code != "hosted_x_retired" || got.Completed || got.CanonicalCalls != 0 || got.Processes != 0 || got.Turns != 0 || fixture.responses != 0 || fixture.failure != "" {
					t.Fatal("retired X selector launched, claimed success or lost its explicit diagnostic")
				}
				return
			}
			if scenario == "unexpected_x_rejected" {
				failure := live.DescribeFailure(err)
				if err == nil || (failure.Code != "hosted_x_retired" && failure.Code != "turn_unsuccessful") || got.Completed || got.FirstCompleted || got.FollowupCompleted || got.CanonicalCalls != 0 || got.SettledTurns != 0 || failure.Code == "turn_unsuccessful" && failure.TerminalStatus != "failed" || !got.Bound || got.Processes != 1 || got.Initializations != 1 || got.Threads != 1 || got.Turns != 1 || got.CleanupPending || fixture.responses != 1 || fixture.failure != "" || ctx.Err() != nil {
					t.Fatalf("unexpected hosted X must fail without dispatch, fabricated credit, SDK ABI rejection or retry: evidence=%+v failure=%+v requests=%d", got, failure, fixture.responses)
				}
				return
			}
			if err != nil || fixture.failure != "" || fixture.responses != 2 || strings.Contains(string(encoded), "PRIVATE_") {
				t.Fatalf("native hosted search: %+v; %v", got, err)
			}
			if !got.Completed || !got.FirstCompleted || !got.FollowupCompleted || got.Scenario != scenario || got.CanonicalCalls != 1 || got.SettledTurns != 2 || got.Turns != 2 || got.Processes != 1 || got.Initializations != 1 || got.Threads != 1 || got.Model != options.Model {
				t.Fatal("native hosted search evidence was not proven")
			}
		})
	}
}

// Backend fixtures reject malformed/empty results without spending another turn.
func TestHostedSearchStructuredReplyFailures(t *testing.T) {
	subject := nativeSubject(t)
	valid := `{"articles":[{"headline":"Fixture headline","url":"https://news.example.test/story"}],"summary":null}`
	for _, test := range []struct {
		name    string
		replies []string
	}{
		{"empty_articles", []string{`{"articles":[],"summary":null}`}},
		{"plain_text", []string{"A headline and URL"}},
		{"duplicate_nested", []string{`{"articles":[{"headline":"a","headline":"b","url":"https://news.example.test"}],"summary":null}`}},
		{"null_continuation", []string{valid, `{"summary":null}`}},
	} {
		t.Run(test.name, func(t *testing.T) {
			integrationObservation(t)
			fixture := &hostedNativeHTTP{scenario: "web", replies: test.replies}
			backend := httptest.NewServer(fixture)
			defer backend.Close()
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
			defer cancel()
			got, err := live.HostedSearch(ctx, live.Options{Subject: subject, Model: providerfixture.PrimaryModel, BaseURL: backend.URL + "/v1", APIKey: "fixture-key"}, "web")
			fixture.mu.Lock()
			defer fixture.mu.Unlock()
			if err == nil || got.Completed || got.FollowupCompleted || fixture.failure != "" || fixture.responses != len(test.replies) || got.Turns != len(test.replies) {
				t.Fatalf("invalid result accepted or retried: %+v %v requests=%d failure=%s", got, err, fixture.responses, fixture.failure)
			}
		})
	}
}
