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
		var script struct{ Mode string }
		_ = json.Unmarshal([]byte(os.Getenv("GROK_API_KEY")), &script)
		if strings.HasPrefix(script.Mode, "history:") {
			fakeHistoryServer()
		} else if strings.HasPrefix(script.Mode, "edit:") {
			fakeEditServer()
		} else {
			fakeAppServer()
		}
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
	_, _ = io.WriteString(os.Stderr, "PRIVATE_CANARY\n")
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
		if script.Mode == "budget" && (request.Method == "initialize" || request.Method == "thread/start") {
			for range 40 {
				_ = output.Encode(map[string]any{"method": "noop", "error": strings.Repeat("x", 120<<10)})
			}
			if request.Method == "thread/start" {
				time.Sleep(time.Hour)
				return
			}
		}
		var result any
		switch request.Method {
		case "initialize":
			result = map[string]any{"userAgent": "fixture"}
			if script.Mode == "bad_init" {
				result = nil
			}
			if script.Mode == "rpc_error" {
				_ = output.Encode(map[string]any{"id": request.ID, "error": map[string]any{"message": "PRIVATE_CANARY"}})
				return
			}
		case "initialized":
			continue
		case "thread/start":
			result = map[string]any{"model": request.Params.Model, "modelProvider": "grok", "thread": map[string]any{"id": "thread", "modelProvider": "grok"}}
			if script.Mode == "wrong_model" {
				result.(map[string]any)["model"] = "PRIVATE_CANARY"
			}
			if script.Mode == "wrong_provider" {
				result.(map[string]any)["modelProvider"] = "PRIVATE_CANARY"
			}
			if script.Mode == "wrong_thread_provider" {
				result.(map[string]any)["thread"].(map[string]any)["modelProvider"] = "PRIVATE_CANARY"
			}
		case "turn/start":
			result = map[string]any{"turn": map[string]any{"id": "turn", "status": "inProgress"}}
		default:
			return
		}
		agent := map[string]any{"type": "agentMessage", "id": "message", "phase": "final_answer", "text": "fixture reply"}
		if script.Mode == "legacy" {
			delete(agent, "phase")
		}
		if script.Mode == "commentary" {
			agent["phase"] = "commentary"
		}
		if script.Mode == "empty" {
			agent["text"] = " "
		}
		item := map[string]any{"method": "item/completed", "params": map[string]any{"threadId": "thread", "turnId": "turn", "item": agent}}
		if request.Method == "turn/start" && script.Mode == "early" {
			_ = output.Encode(item)
		}
		if request.Method == "turn/start" && (script.Mode == "inline_request" || script.Mode == "await_request") {
			_ = output.Encode(map[string]any{"id": 99, "method": "item/tool/call", "params": map[string]any{"tool": "PRIVATE_CANARY"}})
			if script.Mode == "await_request" {
				var rejection struct {
					ID    int
					Error struct{ Code int }
				}
				want := rejection
				want.ID, want.Error.Code = 99, -32601
				if !input.Scan() || json.Unmarshal(input.Bytes(), &rejection) != nil || rejection != want {
					return
				}
				_, _ = io.WriteString(trace, "unsupported_rejected\n")
			}
		}
		if request.Method == "turn/start" && (script.Mode == "inline" || script.Mode == "inline_request" || script.Mode == "await_request") {
			result = map[string]any{"turn": map[string]any{"id": "turn", "status": "completed", "error": nil, "items": []any{agent}}}
		}
		_ = output.Encode(map[string]any{"id": request.ID, "result": result})
		if request.Method == "turn/start" {
			switch script.Mode {
			case "inline", "inline_request", "await_request":
				return
			case "hang":
				time.Sleep(time.Hour)
				return
			case "malformed":
				_, _ = io.WriteString(os.Stdout, "PRIVATE_CANARY\n")
				return
			case "oversized":
				_ = output.Encode(map[string]any{"method": "noop", "params": strings.Repeat("x", 17<<20)})
				time.Sleep(time.Hour)
				return
			case "request":
				_ = output.Encode(map[string]any{"id": 99, "method": "item/tool/call", "params": map[string]any{"tool": "PRIVATE_CANARY"}})
			case "only_reply":
				_ = output.Encode(item)
				return
			}
			items := []any{agent}
			if script.Mode == "item" || script.Mode == "early" || script.Mode == "late" {
				items = nil
			}
			if script.Mode == "item" {
				_ = output.Encode(item)
			}
			if script.Mode == "delta" {
				items = nil
				_ = output.Encode(map[string]any{"method": "item/agentMessage/delta", "params": map[string]any{"threadId": "thread", "turnId": "turn", "delta": "PRIVATE_CANARY"}})
			}
			completed := map[string]any{"id": "turn", "status": "completed", "error": nil, "items": items}
			threadID := "thread"
			if script.Mode == "wrong_thread" {
				threadID = "PRIVATE_CANARY"
			}
			if script.Mode == "wrong_turn" {
				completed["id"] = "PRIVATE_CANARY"
			}
			if script.Mode == "failed" {
				completed["status"] = "failed"
				completed["error"] = map[string]any{"message": "PRIVATE_CANARY"}
			}
			if script.Mode == "partial" {
				completed["status"] = "inProgress"
			}
			_ = output.Encode(map[string]any{"method": "turn/completed", "params": map[string]any{"threadId": threadID, "turn": completed}})
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
	script, _ := json.Marshal(map[string]string{"Mode": mode, "Trace": filepath.Join(t.TempDir(), "rpc")})
	return live.Options{Subject: binarySubject(t, binary), Model: providerfixture.PrimaryModel, BaseURL: "http://fixture.invalid/v1", APIKey: string(script)}
}

func binarySubject(t *testing.T, binary string) live.Subject {
	t.Helper()
	file, err := os.Open(binary)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	hash := sha256.New()
	if _, err := io.Copy(hash, file); err != nil {
		t.Fatal(err)
	}
	return live.Subject{Binary: binary, SHA256: hex.EncodeToString(hash.Sum(nil)), SourceSHA: strings.Repeat("a", 40), HarnessSHA: strings.Repeat("b", 40), Target: "test-host", Environment: "deterministic"}
}

func TestBasicCompletesMatchingTurn(t *testing.T) {
	for _, mode := range []string{"turn", "pinned", "item", "early", "late", "legacy", "inline"} {
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

func TestBasicRejectsInsufficientEvidence(t *testing.T) {
	for mode, stage := range map[string]string{
		"artifact": "preflight", "unavailable": "preflight", "metadata": "preflight", "unbounded": "preflight",
		"wrong_model": "initialized", "wrong_provider": "initialized", "wrong_thread_provider": "initialized",
		"bad_init": "process_started", "rpc_error": "process_started", "budget": "initialized",
		"inline_request": "thread_bound", "await_request": "thread_bound",
		"wrong_thread": "turn_submitted", "wrong_turn": "turn_submitted", "failed": "turn_submitted", "partial": "turn_submitted",
		"request": "turn_submitted", "malformed": "turn_submitted", "oversized": "turn_submitted", "hang": "turn_submitted",
		"empty": "turn_completed", "commentary": "turn_completed", "delta": "turn_completed", "only_reply": "reply_observed",
	} {
		t.Run(mode, func(t *testing.T) {
			options := fixtureOptions(t, mode)
			if mode == "artifact" {
				options.Subject.SHA256 = strings.Repeat("0", 64)
			}
			if mode == "unavailable" {
				options.Subject.Binary += ".PRIVATE_CANARY"
			}
			if mode == "metadata" {
				options.Subject.Environment = "PRIVATE_CANARY/?"
			}
			deadline := 5 * time.Second
			if mode == "hang" {
				deadline = time.Second
			}
			ctx, cancel := context.WithTimeout(context.Background(), deadline)
			defer cancel()
			if mode == "unbounded" {
				ctx = context.Background()
			}
			got, err := live.Basic(ctx, options)
			if err == nil {
				t.Fatal("insufficient evidence accepted")
			}
			encoded, _ := json.Marshal(got)
			if strings.Contains(string(encoded)+err.Error(), "PRIVATE_CANARY") || strings.Contains(string(encoded)+err.Error(), options.Subject.Binary) || strings.Contains(string(encoded)+err.Error(), options.BaseURL) {
				t.Fatal("private diagnostics escaped")
			}
			got.ObservedAt = ""
			want := live.Evidence{SHA256: options.Subject.SHA256, SourceSHA: options.Subject.SourceSHA, HarnessSHA: options.Subject.HarnessSHA, Target: "test-host", Environment: "deterministic", Model: options.Model, Stage: stage, Processes: 1, Initializations: 1, Threads: 1, Turns: 1, Bound: true}
			if stage == "initialized" {
				want.Turns, want.Bound = 0, false
			}
			if stage == "process_started" {
				want.Threads, want.Turns, want.Bound = 0, 0, false
			}
			if stage == "turn_completed" {
				want.Completed = true
			}
			if stage == "reply_observed" {
				want.ReplyBytes = 13
			}
			if stage == "preflight" {
				want.Processes, want.Initializations, want.Threads, want.Turns, want.Bound = 0, 0, 0, 0, false
				if mode == "metadata" || mode == "unbounded" {
					want = live.Evidence{Stage: stage}
				}
				var script struct{ Trace string }
				_ = json.Unmarshal([]byte(options.APIKey), &script)
				if _, err := os.Stat(script.Trace); !os.IsNotExist(err) {
					t.Fatal("preflight launched a child")
				}
			}
			if got != want {
				t.Fatalf("evidence = %+v, want %+v", got, want)
			}
			if mode == "oversized" && err.Error() != "live: protocol frame budget exceeded" {
				t.Fatal("frame cap not observed before EOF")
			}
			if mode == "budget" && err.Error() != "live: early evidence budget exceeded" {
				t.Fatal("retained cap not observed before EOF")
			}
			if mode == "await_request" {
				var script struct{ Trace string }
				_ = json.Unmarshal([]byte(options.APIKey), &script)
				trace, readErr := os.ReadFile(script.Trace)
				if readErr != nil || string(trace) != "process\ninitialize\ninitialized\nthread/start\nturn/start\nunsupported_rejected\n" {
					t.Fatal("server request was not refused before the RPC response")
				}
			}
		})
	}
}
