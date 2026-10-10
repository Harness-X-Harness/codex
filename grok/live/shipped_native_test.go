package live

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
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
)

// Exercise the shipped observer's durable DTO against the actual App Server.
// The controlled HTTP fixture keeps this test independent of backend access.
func TestNativeShippedThreadSource(t *testing.T) {
	binary := os.Getenv("GROK_LIVE_NATIVE_BIN")
	if binary == "" {
		t.Skip("activated by the required native runtime job")
	}
	file, err := os.Open(binary)
	if err != nil {
		t.Fatal(err)
	}
	hash := sha256.New()
	_, err = io.Copy(hash, file)
	closeErr := file.Close()
	if err != nil || closeErr != nil {
		t.Fatal("native executable digest unavailable")
	}
	subject := Subject{
		Binary: binary, SHA256: hex.EncodeToString(hash.Sum(nil)),
		SourceSHA: os.Getenv("GITHUB_SHA"), HarnessSHA: os.Getenv("GITHUB_SHA"),
		Target: "x86_64-unknown-linux-gnu", Environment: "native-fixture",
	}
	var requests atomic.Int32
	var invalid atomic.Bool
	backend := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		defer request.Body.Close()
		requests.Add(1)
		var body struct {
			Model         string
			Tools         []json.RawMessage
			Stream, Store bool
		}
		if json.NewDecoder(io.LimitReader(request.Body, 1<<20)).Decode(&body) != nil ||
			request.Method != "POST" || request.URL.Path != "/v1/responses" ||
			request.Header.Get("Authorization") != "Bearer fixture-key" ||
			body.Model != providerfixture.PrimaryModel || len(body.Tools) != 0 || !body.Stream || body.Store {
			invalid.Store(true)
		}
		writer.Header().Set("Content-Type", "text/event-stream")
		message := map[string]any{"type": "message", "id": "message", "role": "assistant", "phase": "final_answer", "status": "completed",
			"content": []any{map[string]any{"type": "output_text", "text": "native reply"}}}
		for _, event := range []any{
			map[string]any{"type": "response.created", "response": map[string]any{"id": "response"}},
			map[string]any{"type": "response.output_item.added", "output_index": 0,
				"item": map[string]any{"type": "message", "id": "message", "role": "assistant", "phase": "final_answer", "content": []any{}}},
			map[string]any{"type": "response.output_text.delta", "output_index": 0, "delta": "native reply"},
			map[string]any{"type": "response.output_item.done", "output_index": 0, "item": message},
			map[string]any{"type": "response.completed", "response": map[string]any{"id": "response"}},
		} {
			encoded, _ := json.Marshal(event)
			_, _ = fmt.Fprintf(writer, "data: %s\n\n", encoded)
		}
	}))
	defer backend.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	options := Options{Subject: subject, Model: providerfixture.PrimaryModel, BaseURL: backend.URL + "/v1", APIKey: "fixture-key"}
	got, err := runFixture(ctx, options, nil, nil, func(server *appServer, id string, evidence *Evidence) error {
		thread, turn, err := observeShippedTurn(server, id, evidence, "", "Reply with a short confirmation without using tools.")
		if err != nil {
			return err
		}
		if thread.ParentThreadID != "" || thread.ForkedFromID != "" || len(thread.Turns) != 1 || thread.Turns[0].ID != turn.ID {
			t.Error("native root thread binding changed")
		}
		return nil
	})
	if err != nil {
		t.Fatalf("native durable observer: %v", err)
	}
	got.ObservedAt = ""
	want := Evidence{SHA256: subject.SHA256, SourceSHA: subject.SourceSHA, HarnessSHA: subject.HarnessSHA,
		Target: subject.Target, Environment: subject.Environment, Model: providerfixture.PrimaryModel,
		Stage: "product_turn_proven", Processes: 1, Initializations: 1, Threads: 1, Turns: 1, ReplyBytes: 12, Bound: true, Completed: true}
	if got != want || requests.Load() != 1 || invalid.Load() {
		t.Fatalf("native durable evidence mismatch: %+v", got)
	}
}
