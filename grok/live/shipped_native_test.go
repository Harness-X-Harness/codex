package live

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"sync/atomic"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
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
	transport := &sdkProductTransport{ctx: ctx, changed: make(chan struct{}, 1)}
	handlers := sdkFixtureHandlers{
		requests: transport.request, notifications: transport.typedNotification,
		raw: &codexsdk.RawNotificationSubscription{
			Methods:                        []string{codexsdk.RawResponseItemCompletedExtension},
			EnableRawResponseItemExtension: true, Handler: transport.rawNotification,
		},
	}
	got, err := withSDKFixture(ctx, options, nil, handlers, func(client *codexsdk.Client, cwd string, evidence *Evidence) error {
		transport.client = client
		experimental := true
		evidence.Threads = 1
		started, err := client.Threads().Start(ctx, protocolv2.ThreadStartParams{
			Model: protocolv2.Value(options.Model), ModelProvider: protocolv2.Value("grok"), CWD: protocolv2.Value(cwd),
			ExperimentalRawEvents: &experimental, ApprovalPolicy: protocolv2.Value(protocolv2.NewAskForApprovalNever()),
		})
		if err != nil {
			return shippedTransport(ctx, err)
		}
		if started.Model != options.Model || started.ModelProvider != "grok" || started.Thread.ModelProvider != "grok" || started.Thread.ID == "" || started.CWD != cwd {
			return errors.New("live: fixture binding not established")
		}
		evidence.Bound, evidence.Stage = true, "thread_bound"
		thread, turn, err := observeShippedTurn(transport, started.Thread.ID, evidence, "", "Reply with a short confirmation without using tools.")
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
