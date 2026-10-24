//go:build integration

package integration_test

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/integrationfixture"
	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

type firstRequestAttempt struct {
	Method string                               `json:"method"`
	Path   string                               `json:"path"`
	Shape  integrationfixture.FirstRequestShape `json:"shape"`
}

// This is a controlled first-request diagnostic, not real-provider acceptance.
// A synthetic denial ends every case before tool execution or history replay.
func TestFirstRequestShape(t *testing.T) {
	subject := nativeSubject(t)
	for _, model := range []string{providerfixture.PinnedModel, providerfixture.PrimaryModel} {
		profiles := []string{"isolated-history", "shipped-history", "isolated-edit"}
		for _, profile := range profiles {
			t.Run(model+"/"+profile, func(t *testing.T) {
				integrationObservation(t)
				var mu sync.Mutex
				requests := 0
				var attempts []firstRequestAttempt
				valid := true
				backend := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
					defer r.Body.Close()
					mu.Lock()
					defer mu.Unlock()
					requests++
					data, err := io.ReadAll(io.LimitReader(r.Body, (1<<20)+1))
					// Grok's existing sampling retry budget is one. This fixture
					// does not change it or submit another turn to obtain success.
					if requests > 2 {
						valid = false
					} else {
						attempt := firstRequestAttempt{Method: "other", Path: "other"}
						if r.Method == "POST" {
							attempt.Method = "POST"
						}
						if r.URL.Path == "/v1/responses" {
							attempt.Path = "/v1/responses"
						}
						if attempt.Method == "other" || attempt.Path == "other" || r.Header.Get("Authorization") != "Bearer fixture-key" {
							valid = false
						}
						if err != nil || len(data) > 1<<20 {
							valid = false
						} else if got, ok := integrationfixture.SummarizeFirstRequest(data); !ok {
							valid = false
						} else {
							attempt.Shape = got
							var body struct{ Tools []json.RawMessage }
							if json.Unmarshal(data, &body) != nil || !integrationfixture.FirstRequestScenarioTool(body.Tools, profile) {
								valid = false
							}
						}
						attempts = append(attempts, attempt)
					}
					w.Header().Set("Content-Type", "application/json")
					w.WriteHeader(http.StatusForbidden)
					_, _ = io.WriteString(w, `{"error":{"message":"controlled fixture denial","type":"permission_error","code":"fixture_denied"}}`)
				}))
				defer backend.Close()
				options := live.Options{Subject: subject, Model: model, BaseURL: backend.URL + "/v1", APIKey: "fixture-key"}
				ctx, cancel := context.WithTimeout(context.Background(), time.Minute)
				defer cancel()
				if profile == "shipped-history" {
					ctx = withShippedProfile(t, ctx, options.BaseURL)
				}
				var evidence live.Evidence
				var runErr error
				if profile == "isolated-edit" {
					// The controlled denial stops the public scenario in its first turn.
					got, err := live.StructuredEdit(ctx, options)
					evidence, runErr = got.Evidence, err
				} else {
					evidence, runErr = live.ReasoningHistory(ctx, options)
				}
				mu.Lock()
				defer mu.Unlock()
				failure := live.DescribeFailure(runErr)
				encoded, _ := json.Marshal(attempts)
				// Never emit raw request/response bodies, prompts, headers or IDs.
				t.Logf("controlled first request: source=%s harness=%s target=%s executable_sha256=%s profile=%s requests=%d attempts=%s failure=%+v",
					subject.SourceSHA, subject.HarnessSHA, subject.Target, subject.SHA256, profile, requests, encoded, failure)
				if evidence.CleanupPending || runErr == nil || !valid || requests < 1 || requests > 2 || len(attempts) != requests ||
					!evidence.Bound || evidence.Completed || evidence.Turns != 1 || evidence.Processes != 1 || evidence.Threads != 1 {
					t.Fatal("controlled denial must remain unsuccessful in one bound turn within the existing sampling attempt budget")
				}
				if profile != "isolated-edit" && (failure.Code != "turn_unsuccessful" || failure.TerminalStatus != "failed" || failure.HTTPStatus != 403) {
					t.Error("history observer lost its bound failure diagnostics")
				}
				if profile == "isolated-edit" && (runErr.Error() != "live: edit turn did not complete successfully" || failure.Code != "turn_unsuccessful") {
					t.Error("edit request-shape proof must reach the rejected terminal, not a timeout or unrelated error")
				}
				for _, attempt := range attempts {
					shape := attempt.Shape
					if shape.Model != model || !shape.Stream || !integrationfixture.ContainsShapeValue(shape.Include, "reasoning.encrypted_content") {
						t.Error("model, streaming or encrypted-reasoning request contract changed")
					}
					if shape.Store == nil || *shape.Store {
						t.Error("server-side response storage was not explicitly disabled")
					}
					if profile == "isolated-history" {
						if integrationfixture.ContainsShapeValue(shape.ReasoningFields, "effort") {
							t.Error("omitted history effort was reintroduced on the wire")
						}
					} else if profile == "shipped-history" && shape.Effort != "high" {
						t.Error("shipped catalog default effort was not preserved")
					}
				}
				// Other presence/defaults are observations, not guessed backend rules.
			})
		}
	}
}
