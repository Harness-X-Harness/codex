//go:build integration

package integration_test

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/dist"
	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

func TestBasicFixture(t *testing.T) {
	for _, model := range []string{providerfixture.PrimaryModel, providerfixture.PinnedModel} {
		t.Run(model, func(t *testing.T) {
			integrationObservation(t)
			var requests atomic.Int32
			var invalid atomic.Bool
			backend := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
				defer request.Body.Close()
				requests.Add(1)
				var body struct {
					Model  string            `json:"model"`
					Tools  []json.RawMessage `json:"tools"`
					Stream bool
					Store  *bool
				}
				if json.NewDecoder(io.LimitReader(request.Body, 1<<20)).Decode(&body) != nil || request.Method != "POST" || request.URL.Path != "/v1/responses" || request.Header.Get("Authorization") != "Bearer fixture-key" || body.Model != model || len(body.Tools) != 0 || !body.Stream || body.Store == nil || *body.Store {
					invalid.Store(true)
				}
				writer.Header().Set("Content-Type", "text/event-stream")
				message := map[string]any{"type": "message", "id": "message", "role": "assistant", "phase": "final_answer", "status": "completed", "content": []any{map[string]any{"type": "output_text", "text": "native reply"}}}
				for _, event := range []any{
					map[string]any{"type": "response.created", "response": map[string]any{"id": "response"}},
					map[string]any{"type": "response.output_item.added", "output_index": 0, "item": map[string]any{"type": "message", "id": "message", "role": "assistant", "phase": "final_answer", "content": []any{}}},
					map[string]any{"type": "response.output_text.delta", "output_index": 0, "delta": "native reply"},
					map[string]any{"type": "response.output_item.done", "output_index": 0, "item": message},
					map[string]any{"type": "response.completed", "response": map[string]any{"id": "response"}},
				} {
					encoded, _ := json.Marshal(event)
					_, _ = fmt.Fprintf(writer, "data: %s\n\n", encoded)
				}
			}))
			defer backend.Close()
			subject := nativeSubject(t)
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
			defer cancel()
			got, err := live.Basic(ctx, live.Options{Subject: subject, Model: model, BaseURL: backend.URL + "/v1", APIKey: "fixture-key"})
			if err != nil {
				t.Fatalf("native fixture: %+v; %v; requests=%d invalid=%t", got, err, requests.Load(), invalid.Load())
			}
			encoded, _ := json.Marshal(got)
			t.Logf("native fixture evidence: %s", encoded)
			if got.LocalUnverified != subject.LocalUnverified || got.SHA256 != subject.SHA256 ||
				got.SourceSHA != subject.SourceSHA || got.HarnessSHA != subject.HarnessSHA ||
				got.Target != subject.Target || got.Environment != subject.Environment || got.Model != model ||
				!got.Bound || !got.Completed || got.CleanupPending || got.ReplyBytes <= 0 ||
				got.Processes != 1 || got.Initializations != 1 || got.Threads != 1 || got.Turns != 1 ||
				requests.Load() == 0 || invalid.Load() {
				t.Fatal("native Provider/fixture/terminal composition was not proven")
			}
		})
	}
}

func TestShippedCatalog(t *testing.T) {
	integrationObservation(t)
	subject := nativeSubject(t)
	ctx, cancel := context.WithTimeout(context.Background(), time.Minute)
	defer cancel()
	models, err := dist.ModelSlugs()
	if err != nil {
		t.Fatal(err)
	}
	got, err := live.ShippedCatalog(ctx, subject, "catalog-only-fixture")
	if err != nil || !got.ShippedCatalog || got.CatalogModels != len(models) || got.Turns != 0 || got.CleanupPending {
		t.Fatalf("native shipped catalog: %+v %v", got, err)
	}
}
