package live_test

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"sync/atomic"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

func TestNativeBasicFixture(t *testing.T) {
	binary := os.Getenv("GROK_LIVE_NATIVE_BIN")
	if binary == "" {
		t.Skip("native CLI fixture is activated by the required runtime job")
	}
	for _, model := range []string{providerfixture.PrimaryModel, providerfixture.PinnedModel} {
		t.Run(model, func(t *testing.T) {
			var requests atomic.Int32
			var invalid atomic.Bool
			backend := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
				defer request.Body.Close()
				requests.Add(1)
				var body struct {
					Model         string            `json:"model"`
					Tools         []json.RawMessage `json:"tools"`
					Stream, Store bool
				}
				if json.NewDecoder(io.LimitReader(request.Body, 1<<20)).Decode(&body) != nil || request.Method != "POST" || request.URL.Path != "/v1/responses" || request.Header.Get("Authorization") != "Bearer fixture-key" || body.Model != model || len(body.Tools) != 0 || !body.Stream || body.Store {
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
			subject := binarySubject(t, binary)
			subject.SourceSHA, subject.HarnessSHA = os.Getenv("GITHUB_SHA"), os.Getenv("GITHUB_SHA")
			subject.Target, subject.Environment = "x86_64-unknown-linux-gnu", "native-fixture"
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
			defer cancel()
			got, err := live.Basic(ctx, live.Options{Subject: subject, Model: model, BaseURL: backend.URL + "/v1", APIKey: "fixture-key"})
			if err != nil {
				t.Fatalf("native fixture: %+v; %v; requests=%d invalid=%t", got, err, requests.Load(), invalid.Load())
			}
			got.ObservedAt = ""
			want := live.Evidence{SHA256: subject.SHA256, SourceSHA: subject.SourceSHA, HarnessSHA: subject.HarnessSHA, Target: subject.Target, Environment: subject.Environment, Model: model, Stage: "final_reply", Processes: 1, Initializations: 1, Threads: 1, Turns: 1, ReplyBytes: 12, Bound: true, Completed: true}
			if got != want || requests.Load() == 0 || invalid.Load() {
				t.Fatal("native Provider/fixture/terminal composition was not proven")
			}
		})
	}
}
