package live_test

import (
	"bufio"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"runtime"
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
		if strings.HasPrefix(script.Mode, "search:") {
			fakeSearchServer()
		} else if strings.HasPrefix(script.Mode, "shipped:") {
			fakeShippedServer()
		} else if strings.HasPrefix(script.Mode, "history:") {
			fakeHistoryServer()
		} else if strings.HasPrefix(script.Mode, "edit:") {
			fakeEditServer()
		} else if strings.HasPrefix(script.Mode, "image:") {
			fakeImageServer()
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
	if script.Mode == "initialize_stall" {
		signal.Ignore(os.Interrupt)
	}
	// These peers must consume the denial already written by the SDK before
	// normal cleanup interrupts the process. Other fixture modes retain their
	// normal signal behavior. Bound even a broken shutdown to avoid leaked peers.
	denialMode := script.Mode == "unauthorized" || script.Mode == "unauthorized_then_failed" || script.Mode == "unauthorized_shutdown_before_read"
	shutdown := make(chan os.Signal, 1)
	var denialDeadline *time.Timer
	defer func() {
		if denialDeadline != nil {
			denialDeadline.Stop()
		}
	}()
	if denialMode {
		signal.Notify(shutdown, os.Interrupt)
		defer signal.Stop(shutdown)
	}
	trace, err := os.OpenFile(script.Trace, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
	if err != nil {
		return
	}
	defer trace.Close()
	_, _ = io.WriteString(trace, "process\n")
	_ = os.WriteFile(script.Trace+".home", []byte(os.Getenv("CODEX_HOME")), 0600)
	if executable, err := os.Executable(); err == nil {
		_ = os.WriteFile(script.Trace+".executable", []byte(executable), 0600)
	}
	_, _ = io.WriteString(os.Stderr, "PRIVATE_CANARY\n")
	input := bufio.NewScanner(os.Stdin)
	output := json.NewEncoder(os.Stdout)
	recordDenial := func(data []byte) {
		var response struct {
			ID     int
			Error  *struct{ Code int }
			Result *struct{ Success *bool }
		}
		if json.Unmarshal(data, &response) != nil || response.ID != 99 {
			_, _ = io.WriteString(trace, "unauthorized_invalid\n")
			return
		}
		if response.Result != nil && response.Result.Success != nil && *response.Result.Success {
			_ = os.WriteFile(script.Trace+".effect", []byte("unauthorized"), 0600)
		} else if (response.Error != nil && response.Error.Code < 0 && response.Result == nil) ||
			(response.Error == nil && response.Result != nil && response.Result.Success != nil && !*response.Result.Success) {
			_, _ = io.WriteString(trace, "unauthorized_denied\n")
		} else {
			_, _ = io.WriteString(trace, "unauthorized_invalid\n")
		}
	}
	drainDenials := func() {
		for input.Scan() {
			recordDenial(input.Bytes())
		}
		if input.Err() == nil {
			_, _ = io.WriteString(trace, "unauthorized_eof\n")
		}
	}
	unauthorized := func() {
		denialDeadline = time.AfterFunc(1500*time.Millisecond, func() { os.Exit(2) })
		_ = output.Encode(map[string]any{"id": 99, "method": "item/tool/call", "params": map[string]any{
			"threadId": "thread", "turnId": "turn", "callId": "call", "tool": "PRIVATE_CANARY", "arguments": map[string]any{},
		}})
		if script.Mode == "unauthorized_shutdown_before_read" {
			select {
			case <-shutdown:
				_, _ = io.WriteString(trace, "unauthorized_shutdown\n")
			case <-time.After(time.Second):
				return
			}
		}
		if input.Scan() {
			recordDenial(input.Bytes())
		}
	}
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
		if script.Mode == "budget" && request.Method == "thread/start" {
			for range 256 {
				_ = output.Encode(map[string]any{"method": "item/completed", "params": map[string]any{
					"threadId": "thread", "turnId": "turn", "completedAtMs": 1,
					"item": map[string]any{"type": "agentMessage", "id": "message", "phase": "final_answer", "text": "fixture reply"},
				}})
			}
			time.Sleep(time.Hour)
			return
		}
		var result any
		switch request.Method {
		case "initialize":
			if script.Mode == "initialize_stall" {
				// The command returns on its startup deadline without a join handle.
				// The test releases this bounded helper only after that return.
				limit := time.Now().Add(8 * time.Second)
				for time.Now().Before(limit) {
					if _, err := os.Stat(script.Trace + ".release"); err == nil {
						break
					}
					time.Sleep(10 * time.Millisecond)
				}
				executable, executableErr := os.Executable()
				_, binaryErr := os.Stat(executable)
				_, homeErr := os.Stat(os.Getenv("CODEX_HOME"))
				value := fmt.Sprintf("missing: os.Executable path=%q error=%v; executable stat error=%v; CODEX_HOME path=%q stat error=%v",
					executable, executableErr, binaryErr, os.Getenv("CODEX_HOME"), homeErr)
				if executableErr == nil && binaryErr == nil && homeErr == nil {
					value = "retained"
				}
				// File existence signals a complete observation to the command test.
				pending := script.Trace + ".retention.tmp"
				if err := os.WriteFile(pending, []byte(value), 0600); err != nil {
					return
				}
				if err := os.Rename(pending, script.Trace+".retention"); err != nil {
					return
				}
				return
			}
			reportedHome, _ := filepath.EvalSymlinks(os.Getenv("CODEX_HOME"))
			result = map[string]any{"userAgent": "fixture", "codexHome": reportedHome, "platformFamily": "unix", "platformOs": "linux"}
			if script.Mode == "wrong_home" {
				result.(map[string]any)["codexHome"] = filepath.Dir(reportedHome)
			}
			if script.Mode == "bad_init" {
				result = nil
			}
			if script.Mode == "rpc_error" {
				_ = output.Encode(map[string]any{"id": request.ID, "error": map[string]any{"code": -32000, "message": "PRIVATE_CANARY"}})
				return
			}
		case "initialized":
			continue
		case "thread/start":
			result = map[string]any{
				"model": request.Params.Model, "modelProvider": "grok", "approvalPolicy": "never", "approvalsReviewer": "user", "cwd": os.Getenv("CODEX_HOME"), "sandbox": map[string]any{"type": "readOnly"},
				"thread": map[string]any{"id": "thread", "sessionId": "session", "modelProvider": "grok", "cliVersion": "fixture", "createdAt": 1, "updatedAt": 1, "cwd": os.Getenv("CODEX_HOME"), "ephemeral": false, "preview": "", "projectId": nil, "source": "appServer", "status": map[string]any{"type": "idle"}, "turns": []any{}},
			}
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
			result = map[string]any{"turn": map[string]any{"id": "turn", "status": "inProgress", "items": []any{}}}
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
		item := map[string]any{"method": "item/completed", "params": map[string]any{"threadId": "thread", "turnId": "turn", "item": agent, "completedAtMs": 1}}
		if request.Method == "turn/start" && script.Mode == "callback_budget" {
			for range 129 {
				_ = output.Encode(item)
			}
			time.Sleep(time.Hour)
			return
		}
		if request.Method == "turn/start" && script.Mode == "early" {
			_ = output.Encode(item)
		}
		if request.Method == "turn/start" && (script.Mode == "inline_request" || script.Mode == "await_request") {
			_ = output.Encode(map[string]any{"id": 99, "method": "item/tool/call", "params": map[string]any{"tool": "PRIVATE_CANARY"}})
			if script.Mode == "await_request" {
				var rejection struct {
					ID     int
					Error  *struct{ Code int }
					Result json.RawMessage
				}
				if !input.Scan() || json.Unmarshal(input.Bytes(), &rejection) != nil || rejection.ID != 99 || rejection.Error == nil || rejection.Error.Code >= 0 || rejection.Result != nil {
					return
				}
				_, _ = io.WriteString(trace, "unsupported_rejected\n")
			}
		}
		if request.Method == "turn/start" && (script.Mode == "inline" || script.Mode == "inline_request" || script.Mode == "await_request") {
			result = map[string]any{"turn": map[string]any{"id": "turn", "status": "completed", "error": nil, "items": []any{agent}}}
		}
		if request.Method == "turn/start" && script.Mode == "unauthorized_then_failed" {
			unauthorized()
			result = map[string]any{"turn": map[string]any{"id": "turn", "status": "failed", "error": map[string]any{"message": "PRIVATE_CANARY"}, "items": []any{}}}
		}
		_ = output.Encode(map[string]any{"id": request.ID, "result": result})
		if request.Method == "turn/start" {
			switch script.Mode {
			case "inline", "inline_request", "await_request":
				for input.Scan() {
				}
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
			case "retention_budget":
				agent["text"] = strings.Repeat("x", 3<<20)
				_ = output.Encode(item)
				time.Sleep(time.Hour)
				return
			case "request":
				_ = output.Encode(map[string]any{"id": 99, "method": "item/tool/call", "params": map[string]any{"tool": "PRIVATE_CANARY"}})
			case "unauthorized", "unauthorized_shutdown_before_read":
				unauthorized()
				drainDenials()
				return
			case "only_reply":
				_ = output.Encode(item)
				return
			}
			items := []any{agent}
			if script.Mode == "item" || script.Mode == "early" || script.Mode == "late" {
				items = []any{}
			}
			if script.Mode == "item" {
				_ = output.Encode(item)
			}
			if script.Mode == "delta" {
				items = []any{}
				_ = output.Encode(map[string]any{"method": "item/agentMessage/delta", "params": map[string]any{"threadId": "thread", "turnId": "turn", "itemId": "message", "delta": "PRIVATE_CANARY"}})
			}
			completed := map[string]any{"id": "turn", "status": "completed", "error": nil, "items": items}
			threadID := "thread"
			if script.Mode == "wrong_thread" || script.Mode == "unrelated_failed" {
				threadID = "PRIVATE_CANARY"
			}
			if script.Mode == "wrong_turn" {
				completed["id"] = "PRIVATE_CANARY"
			}
			if script.Mode == "failed" || script.Mode == "failed_typed" || script.Mode == "unrelated_failed" || script.Mode == "unauthorized_then_failed" {
				completed["status"] = "failed"
				completed["error"] = map[string]any{"message": "PRIVATE_CANARY"}
				if script.Mode == "failed_typed" || script.Mode == "unrelated_failed" {
					completed["error"] = map[string]any{"message": "PRIVATE_CANARY", "additionalDetails": "PRIVATE_CANARY", "codexErrorInfo": map[string]any{"responseStreamDisconnected": map[string]any{"httpStatusCode": 429}}}
				}
			}
			if script.Mode == "partial" {
				completed["status"] = "inProgress"
			}
			_ = output.Encode(map[string]any{"method": "turn/completed", "params": map[string]any{"threadId": threadID, "turn": completed}})
			// Wrong-identity controls test attribution at EOF. Missing-final
			// controls keep the stream open so EOF cannot mask false acceptance.
			if script.Mode == "unrelated_failed" || script.Mode == "wrong_thread" || script.Mode == "wrong_turn" {
				return
			}
			if script.Mode == "late" {
				_ = output.Encode(item)
			}
			if denialMode {
				drainDenials()
			} else {
				for input.Scan() {
				}
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
	for _, mode := range []string{"turn", "pinned", "item", "early", "late", "legacy", "inline", "inline_request", "await_request", "request"} {
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
			wantTrace := "process\ninitialize\ninitialized\nthread/start\nturn/start\n"
			if mode == "await_request" {
				wantTrace += "unsupported_rejected\n"
			}
			if string(trace) != wantTrace {
				t.Fatalf("unexpected invocation transcript: %q", trace)
			}
			childHome, err := os.ReadFile(script.Trace + ".home")
			if err != nil || len(childHome) == 0 || string(childHome) == os.Getenv("CODEX_HOME") {
				t.Fatal("child fixture home was not isolated")
			}
			if _, err := os.Stat(string(childHome)); !os.IsNotExist(err) {
				t.Fatal("child fixture survived SDK cleanup")
			}
		})
	}
}

func TestBasicDeniesUnauthorizedAction(t *testing.T) {
	modes := []string{"unauthorized", "unauthorized_then_failed"}
	// os.Process.Signal(os.Interrupt) is unsupported on Windows.
	if runtime.GOOS != "windows" {
		modes = append(modes, "unauthorized_shutdown_before_read")
	}
	for _, mode := range modes {
		t.Run(mode, func(t *testing.T) {
			options := fixtureOptions(t, mode)
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			got, err := live.Basic(ctx, options)
			if err == nil || live.DescribeFailure(err).Code != "unsupported_server_request" || got.Completed || got.CleanupPending {
				t.Fatalf("unauthorized action accepted or first cause lost: %+v; %v", got, err)
			}
			var script struct{ Trace string }
			_ = json.Unmarshal([]byte(options.APIKey), &script)
			if _, err := os.Stat(script.Trace + ".effect"); !os.IsNotExist(err) {
				t.Fatal("unauthorized action produced an effect")
			}
			trace, err := os.ReadFile(script.Trace)
			if err != nil || strings.Count(string(trace), "unauthorized_denied\n") != 1 || strings.Contains(string(trace), "unauthorized_invalid\n") || strings.Count(string(trace), "unauthorized_eof\n") != 1 || strings.Count(string(trace), "turn/start\n") != 1 {
				t.Fatalf("valid unauthorized action was not denied exactly once before clean EOF: %q", trace)
			}
			if mode == "unauthorized_shutdown_before_read" && (strings.Index(string(trace), "unauthorized_shutdown\n") < 0 || strings.Index(string(trace), "unauthorized_shutdown\n") > strings.Index(string(trace), "unauthorized_denied\n")) {
				t.Fatal("denial was not consumed after the controlled shutdown signal")
			}
		})
	}
}

func TestBasicProcessIsolation(t *testing.T) {
	if os.Getenv("GROK_LIVE_ISOLATION_TEST") == "1" {
		options := fixtureOptions(t, "turn")
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		if _, err := live.Basic(ctx, options); err != nil {
			t.Fatal(err)
		}
		return
	}
	parentHome := t.TempDir()
	tempParent, tempAlias := t.TempDir(), filepath.Join(t.TempDir(), "alias")
	if err := os.Symlink(tempParent, tempAlias); err != nil {
		tempAlias = tempParent
		t.Log("symlinks unavailable; exercising ordinary directory isolation")
	}
	profile := filepath.Join(parentHome, "config.toml")
	if err := os.WriteFile(profile, []byte("PRIVATE_PARENT_PROFILE"), 0600); err != nil {
		t.Fatal(err)
	}
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	command := exec.CommandContext(ctx, binary, "-test.run=^TestBasicProcessIsolation$")
	for _, entry := range os.Environ() {
		name, _, _ := strings.Cut(entry, "=")
		if !strings.EqualFold(name, "CODEX_HOME") && !strings.EqualFold(name, "GROK_API_KEY") && name != "GROK_LIVE_ISOLATION_TEST" && name != "TMPDIR" && name != "TMP" && name != "TEMP" {
			command.Env = append(command.Env, entry)
		}
	}
	command.Env = append(command.Env, "CODEX_HOME="+parentHome, "GROK_API_KEY=PRIVATE_PARENT_KEY", "GROK_LIVE_ISOLATION_TEST=1", "TMPDIR="+tempAlias, "TMP="+tempAlias, "TEMP="+tempAlias)
	if output, err := command.CombinedOutput(); err != nil {
		t.Fatalf("isolated process failed: %v; %s", err, output)
	}
	if got, err := os.ReadFile(profile); err != nil || string(got) != "PRIVATE_PARENT_PROFILE" {
		t.Fatal("SDK fixture modified inherited product configuration")
	}
	entries, err := os.ReadDir(parentHome)
	if err != nil || len(entries) != 1 {
		t.Fatal("SDK fixture wrote into inherited product home")
	}
}

func TestBasicRejectsInsufficientEvidence(t *testing.T) {
	for mode, stage := range map[string]string{
		"artifact": "preflight", "unavailable": "preflight", "metadata": "preflight", "unbounded": "preflight",
		"wrong_model": "initialized", "wrong_provider": "initialized", "wrong_thread_provider": "initialized",
		"bad_init": "sdk_starting", "rpc_error": "sdk_starting", "budget": "initialized", "wrong_home": "initialized_home",
		"wrong_thread": "turn_submitted", "wrong_turn": "turn_submitted", "failed": "turn_submitted", "partial": "turn_submitted",
		"malformed": "turn_submitted", "oversized": "turn_submitted", "hang": "turn_submitted", "callback_budget": "turn_submitted", "retention_budget": "turn_submitted",
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
			if mode == "hang" || mode == "empty" || mode == "commentary" || mode == "delta" {
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
			if (mode == "empty" || mode == "commentary" || mode == "delta") && live.DescribeFailure(err).Code != "deadline_exceeded" {
				t.Fatal("missing final reply was not rejected before cleanup")
			}
			if mode == "callback_budget" && live.DescribeFailure(err).Code != "early_evidence_budget_exceeded" {
				t.Fatal("application callback overload was not bounded")
			}
			if mode == "retention_budget" && live.DescribeFailure(err).Code != "notification_retention_exceeded" {
				t.Fatal("SDK retention overload was not bounded")
			}
			encoded, _ := json.Marshal(got)
			if strings.Contains(string(encoded)+err.Error(), "PRIVATE_CANARY") || strings.Contains(string(encoded)+err.Error(), options.Subject.Binary) || strings.Contains(string(encoded)+err.Error(), options.BaseURL) {
				t.Fatal("private diagnostics escaped")
			}
			// SDK timing may change transient transport stages; identity, action
			// counts and credited business evidence remain the acceptance boundary.
			got.ObservedAt, got.Stage = "", ""
			want := live.Evidence{SHA256: options.Subject.SHA256, SourceSHA: options.Subject.SourceSHA, HarnessSHA: options.Subject.HarnessSHA, Target: "test-host", Environment: "deterministic", Model: options.Model, Processes: 1, Initializations: 1, Threads: 1, Turns: 1, Bound: true}
			if stage == "initialized" {
				want.Turns, want.Bound = 0, false
			}
			if stage == "initialized_home" {
				want.Threads, want.Turns, want.Bound = 0, 0, false
			}
			if stage == "sdk_starting" {
				want.CleanupPending = true
				want.Processes, want.Initializations, want.Threads, want.Turns, want.Bound = 0, 0, 0, 0, false
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
					want = live.Evidence{}
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
		})
	}
}
