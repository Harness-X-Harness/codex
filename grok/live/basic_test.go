package live_test

import (
	"bufio"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
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
	var script struct {
		Mode  string
		Trace string
	}
	if json.Unmarshal([]byte(os.Getenv("GROK_API_KEY")), &script) != nil {
		return
	}
	trace, err := os.OpenFile(script.Trace, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
	if err != nil {
		return
	}
	defer trace.Close()
	_, _ = io.WriteString(trace, "process\n")
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
		_, _ = io.WriteString(trace, request.Method+"\n")
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
		agent := map[string]any{"type": "agentMessage", "id": "message", "phase": "final_answer", "text": "fixture reply"}
		if script.Mode == "legacy" {
			delete(agent, "phase")
		}
		item := map[string]any{"method": "item/completed", "params": map[string]any{"threadId": "thread", "turnId": "turn", "item": agent}}
		if request.Method == "turn/start" && script.Mode == "early" {
			_ = output.Encode(item)
		}
		_ = output.Encode(map[string]any{"id": request.ID, "result": result})
		if request.Method == "turn/start" {
			items := []any{agent}
			if script.Mode == "item" || script.Mode == "early" || script.Mode == "late" {
				items = nil
			}
			if script.Mode == "item" {
				_ = output.Encode(item)
			}
			_ = output.Encode(map[string]any{"method": "turn/completed", "params": map[string]any{"threadId": "thread", "turn": map[string]any{"id": "turn", "status": "completed", "error": nil, "items": items}}})
			if script.Mode == "late" {
				_ = output.Encode(item)
			}
			return
		}
	}
}

func fixtureOptions(t *testing.T, mode string) live.Options {
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
	script, _ := json.Marshal(map[string]string{"Mode": mode, "Trace": filepath.Join(t.TempDir(), "rpc")})
	return live.Options{
		Subject: live.Subject{Binary: binary, SHA256: hex.EncodeToString(hash.Sum(nil)), SourceSHA: strings.Repeat("a", 40), HarnessSHA: strings.Repeat("b", 40), Target: "test-host", Environment: "deterministic"},
		Model:   providerfixture.PrimaryModel, BaseURL: "http://fixture.invalid/v1", APIKey: string(script),
	}
}

func TestBasicCompletesMatchingTurn(t *testing.T) {
	for _, mode := range []string{"turn", "pinned", "item", "early", "late", "legacy"} {
		t.Run(mode, func(t *testing.T) {
			options := fixtureOptions(t, mode)
			if mode == "pinned" {
				options.Model = providerfixture.PinnedModel
			}
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
			want := live.Evidence{SHA256: options.Subject.SHA256, SourceSHA: options.Subject.SourceSHA, HarnessSHA: options.Subject.HarnessSHA, Target: "test-host", Environment: "deterministic", Model: options.Model, Stage: "final_reply", Processes: 1, Initializations: 1, Threads: 1, Turns: 1, ReplyBytes: 13, Bound: true, Completed: true}
			if got != want {
				t.Fatalf("evidence = %+v, want %+v", got, want)
			}
			var script struct{ Trace string }
			_ = json.Unmarshal([]byte(options.APIKey), &script)
			trace, err := os.ReadFile(script.Trace)
			if err != nil {
				t.Fatal(err)
			}
			if string(trace) != "process\ninitialize\ninitialized\nthread/start\nturn/start\n" {
				t.Fatalf("unexpected invocation transcript: %q", trace)
			}
		})
	}
}
