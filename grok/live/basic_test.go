package live_test

import (
	"bufio"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

func TestMain(m *testing.M) {
	if len(os.Args) > 1 && os.Args[1] == "app-server" {
		fakeAppServer()
		os.Exit(0)
	}
	os.Exit(m.Run())
}

func fakeAppServer() {
	input := bufio.NewScanner(os.Stdin)
	output := json.NewEncoder(os.Stdout)
	for input.Scan() {
		var request struct {
			ID     json.RawMessage `json:"id"`
			Method string          `json:"method"`
			Params struct {
				Model string `json:"model"`
			} `json:"params"`
		}
		if json.Unmarshal(input.Bytes(), &request) != nil {
			return
		}
		var result any
		switch request.Method {
		case "initialize":
			result = map[string]any{"userAgent": "fixture"}
		case "initialized":
			continue
		case "thread/start":
			result = map[string]any{"model": request.Params.Model, "modelProvider": "grok", "thread": map[string]any{"id": "thread", "modelProvider": "grok"}}
		case "turn/start":
			result = map[string]any{"turn": map[string]any{"id": "turn", "status": "inProgress"}}
		default:
			return
		}
		_ = output.Encode(map[string]any{"id": request.ID, "result": result})
		if request.Method == "turn/start" {
			_ = output.Encode(map[string]any{"method": "turn/completed", "params": map[string]any{"threadId": "thread", "turn": map[string]any{"id": "turn", "status": "completed", "error": nil, "items": []any{map[string]any{"type": "agentMessage", "id": "message", "phase": "final_answer", "text": "fixture reply"}}}}})
		}
	}
}

func fixtureOptions(t *testing.T) live.Options {
	t.Helper()
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	file, err := os.Open(binary)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	hash := sha256.New()
	if _, err := io.Copy(hash, file); err != nil {
		t.Fatal(err)
	}
	return live.Options{
		Subject: live.Subject{Binary: binary, SHA256: hex.EncodeToString(hash.Sum(nil)), SourceSHA: strings.Repeat("a", 40), HarnessSHA: strings.Repeat("b", 40), Target: "test-host", Environment: "deterministic"},
		Model:   providerfixture.PrimaryModel, BaseURL: "http://fixture.invalid/v1", APIKey: "fixture-key",
	}
}

func TestBasicCompletesMatchingTurn(t *testing.T) {
	options := fixtureOptions(t)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	got, err := live.Basic(ctx, options)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := time.Parse(time.RFC3339, got.ObservedAt); err != nil {
		t.Fatal("missing observation time")
	}
	got.ObservedAt = ""
	want := live.Evidence{SHA256: options.Subject.SHA256, SourceSHA: options.Subject.SourceSHA, HarnessSHA: options.Subject.HarnessSHA, Target: "test-host", Environment: "deterministic", Model: providerfixture.PrimaryModel, Stage: "final_reply", Processes: 1, Initializations: 1, Threads: 1, Turns: 1, ReplyBytes: 13, Bound: true, Completed: true}
	if got != want {
		t.Fatalf("evidence = %+v, want %+v", got, want)
	}
}
