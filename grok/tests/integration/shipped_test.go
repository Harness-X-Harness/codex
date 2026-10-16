//go:build integration

package integration_test

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/integrationfixture"
	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/internal/testkit"
	"github.com/Harness-X-Harness/codex/grok/live"
	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// The public scenario and an independent typed thread/read observation both
// consume the same shipped bytes. No private live observer is exported for tests.
func TestShippedThreadSource(t *testing.T) {
	integrationObservation(t)
	subject := nativeSubject(t)
	var requests atomic.Int32
	var invalid atomic.Bool
	backend := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		defer r.Body.Close()
		requests.Add(1)
		var body struct {
			Model         string
			Tools         []json.RawMessage
			Stream, Store bool
		}
		if json.NewDecoder(io.LimitReader(r.Body, 1<<20)).Decode(&body) != nil || r.Method != "POST" || r.URL.Path != "/v1/responses" || r.Header.Get("Authorization") != "Bearer fixture-key" || body.Model != providerfixture.PrimaryModel || !body.Stream || body.Store {
			invalid.Store(true)
		}
		for _, raw := range body.Tools {
			var tool struct{ Type string }
			if json.Unmarshal(raw, &tool) != nil || tool.Type == "x_search" {
				invalid.Store(true)
			}
		}
		events := []any{map[string]any{"type": "response.created", "response": map[string]any{"id": "response"}}}
		events = append(events, assistantEvents(0, "message", "native reply")...)
		events = append(events, map[string]any{"type": "response.completed", "response": map[string]any{"id": "response"}})
		if err := writeSSE(w, events); err != nil {
			invalid.Store(true)
		}
	}))
	defer backend.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	ctx = withShippedProfile(t, ctx, backend.URL+"/v1")
	got, err := live.ShippedStartup(ctx, subject, "fixture-key")
	if err != nil {
		t.Fatalf("shipped durable observer: %v", err)
	}
	got.ObservedAt = ""
	want := live.Evidence{LocalUnverified: subject.LocalUnverified, SHA256: subject.SHA256, SourceSHA: subject.SourceSHA, HarnessSHA: subject.HarnessSHA,
		Target: subject.Target, Environment: subject.Environment, Model: providerfixture.PrimaryModel,
		Stage: "product_turn_proven", Processes: 1, Initializations: 1, Threads: 1, Turns: 1, ReplyBytes: 12, Bound: true, Completed: true, ShippedCatalog: true, CatalogModels: 2}
	if got != want || requests.Load() != 1 || invalid.Load() {
		t.Fatalf("shipped durable evidence mismatch: %+v", got)
	}

	// Read the root provenance through the official SDK on unmodified stdout.
	// Retain inputs if startup or teardown cannot prove ownership has ended.
	home, err := os.MkdirTemp("", "grok-integration-")
	if err != nil {
		t.Fatal("isolated workspace unavailable")
	}
	joined := true
	defer func() {
		if joined {
			_ = os.RemoveAll(home)
		}
	}()
	cwd := filepath.Join(home, "workspace")
	if err := integrationfixture.WriteShippedProfile(home, cwd); err != nil {
		t.Fatal(err)
	}
	if err := integrationfixture.SubstituteShippedEndpoint(home, backend.URL+"/v1"); err != nil {
		t.Fatal(err)
	}
	terminal := make(chan protocolv2.TurnCompletedNotification, 8)
	var unexpected atomic.Bool
	var threadID, turnID string
	experimental := true
	joined = false
	client, err := codexsdk.NewContext(ctx, codexsdk.ClientOptions{
		CWD: cwd, Command: []string{subject.Binary, "app-server", "--strict-config", "--listen", "stdio://"},
		Env:        integrationfixture.Environment(os.Environ(), home, "fixture-key"),
		Initialize: protocolv2.InitializeParams{ClientInfo: protocolv2.ClientInfo{Name: "grok-integration", Version: "1"}, Capabilities: protocolv2.Value(protocolv2.InitializeCapabilities{ExperimentalAPI: &experimental})},
		ServerRequestHandler: func(context.Context, protocolv2.ServerRequest) (codexsdk.ServerRequestResponse, error) {
			unexpected.Store(true)
			return codexsdk.ServerRequestResponse{}, nil
		},
		ServerNotificationHandler: func(_ context.Context, n protocolv2.ServerNotification) error {
			if completed, ok := n.AsTurnCompleted(); ok {
				select {
				case terminal <- completed.Params:
				default:
					unexpected.Store(true)
				}
			}
			return nil
		},
	})
	if err != nil {
		t.Fatal("official SDK startup failed")
	}
	defer func() {
		closeCtx, stop := context.WithTimeout(context.Background(), 5*time.Second)
		defer stop()
		closed, err := client.CloseContext(closeCtx)
		joined = closed.ProcessReaped && closed.CallbackWorkPending == 0
		if unexpected.Load() {
			t.Error("unexpected request or terminal buffer overflow")
		}
		for len(terminal) > 0 {
			completed := <-terminal
			if completed.ThreadID != threadID || completed.Turn.ID != turnID || completed.Turn.Status != protocolv2.TurnStatusCompleted || completed.Turn.Error != nil && completed.Turn.Error.Value != nil {
				t.Error("late contradictory terminal changed durable source evidence")
			}
		}
		if err != nil || !joined {
			t.Error("official SDK process/callback cleanup incomplete")
		}
		digest, err := testkit.ExecutableSHA256(subject.Binary)
		if err != nil || digest != subject.SHA256 {
			t.Error("selected executable changed")
		}
	}()
	identity := client.Provenance().RuntimeAppServer
	actual, ae := os.Stat(identity.CodexHome)
	expected, ee := os.Stat(home)
	if !identity.Observed || identity.UserAgent == "" || ae != nil || ee != nil || !os.SameFile(actual, expected) {
		t.Fatal("SDK initialization identity not established")
	}
	started, err := client.Threads().Start(ctx, protocolv2.ThreadStartParams{Model: protocolv2.Value(providerfixture.PrimaryModel), ModelProvider: protocolv2.Value("grok"), CWD: protocolv2.Value(cwd), ApprovalPolicy: protocolv2.Value(protocolv2.NewAskForApprovalNever())})
	if err != nil {
		t.Fatal("typed thread start failed")
	}
	if started.Model != providerfixture.PrimaryModel || started.ModelProvider != "grok" || started.Thread.ModelProvider != "grok" || started.Thread.ID == "" || started.CWD != cwd {
		t.Fatal("typed thread binding missing")
	}
	threadID = started.Thread.ID
	turn, err := client.Turns().Start(ctx, protocolv2.TurnStartParams{ThreadID: started.Thread.ID, Input: []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{Text: "Reply with a short confirmation without using tools."})}})
	if err != nil || turn.Turn.ID == "" {
		t.Fatal("typed turn start failed")
	}
	turnID = turn.Turn.ID
	if turn.Turn.Status == protocolv2.TurnStatusInProgress {
		select {
		case completed := <-terminal:
			if completed.ThreadID != started.Thread.ID || completed.Turn.ID != turn.Turn.ID || completed.Turn.Status != protocolv2.TurnStatusCompleted || completed.Turn.Error != nil && completed.Turn.Error.Value != nil {
				t.Fatal("bound completed turn absent")
			}
		case <-ctx.Done():
			t.Fatal("typed terminal not observed")
		}
	} else if turn.Turn.Status != protocolv2.TurnStatusCompleted || turn.Turn.Error != nil && turn.Turn.Error.Value != nil {
		t.Fatal("typed turn unsuccessful")
	}
	include := true
	read, err := client.Threads().Read(ctx, protocolv2.ThreadReadParams{ThreadID: started.Thread.ID, IncludeTurns: &include})
	if err != nil {
		t.Fatal("typed durable thread read failed")
	}
	thread := read.Thread
	if thread.ID != started.Thread.ID || thread.ModelProvider != "grok" || thread.Model == nil || thread.Model.Value == nil || *thread.Model.Value != providerfixture.PrimaryModel || thread.Source.Kind() != protocolv2.SessionSourceKindAppServer || thread.ParentThreadID != nil && thread.ParentThreadID.Value != nil && *thread.ParentThreadID.Value != "" || thread.ForkedFromID != nil && thread.ForkedFromID.Value != nil && *thread.ForkedFromID.Value != "" || len(thread.Turns) != 1 || thread.Turns[0].ID != turn.Turn.ID || thread.Turns[0].Status != protocolv2.TurnStatusCompleted || thread.Turns[0].Error != nil && thread.Turns[0].Error.Value != nil {
		t.Fatal("durable root source, model, lineage or turn identity changed")
	}
	reply := ""
	for _, item := range thread.Turns[0].Items {
		if message, ok := item.AsAgentMessage(); ok {
			reply += message.Text
		}
	}
	if reply != "native reply" || requests.Load() != 2 || invalid.Load() || unexpected.Load() {
		t.Fatal("durable reply or controlled request composition changed")
	}
}
