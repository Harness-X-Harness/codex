package live_test

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/signal"
	"reflect"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/live"
)

// This fake speaks only the supported public product protocol. Hosted raw
// envelope/replay compatibility remains in the controlled HTTP integration tests.
func fakeSearchServer() {
	var script struct{ Mode, Trace string }
	_ = json.Unmarshal([]byte(os.Getenv("GROK_API_KEY")), &script)
	parts := strings.Split(script.Mode, ":")
	mode, scenario := parts[1], parts[2]
	trace, err := os.OpenFile(script.Trace, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
	if err != nil {
		return
	}
	defer trace.Close()
	signal.Notify(make(chan os.Signal, 1), os.Interrupt)
	input, output := bufio.NewScanner(os.Stdin), json.NewEncoder(os.Stdout)
	var turns []any
	for input.Scan() {
		var request struct {
			ID     json.RawMessage
			Method string
			Params map[string]any
		}
		if json.Unmarshal(input.Bytes(), &request) != nil {
			return
		}
		_, _ = io.WriteString(trace, request.Method+"\n")
		var result any
		switch request.Method {
		case "initialize":
			family := "unix"
			if runtime.GOOS == "windows" {
				family = "windows"
			}
			result = map[string]any{"userAgent": "fixture", "codexHome": os.Getenv("CODEX_HOME"), "platformFamily": family, "platformOs": runtime.GOOS}
		case "initialized":
			continue
		case "thread/start":
			config := map[string]any{"web_search": "live"}
			if scenario == "web_allowed" {
				config["tools.web_search.allowed_domains"] = []any{"reuters.com"}
			}
			if scenario == "web_excluded" {
				config["tools.web_search.excluded_domains"] = []any{"example.com"}
			}
			if !reflect.DeepEqual(request.Params["config"], config) || request.Params["experimentalRawEvents"] == true {
				return
			}
			cwd, _ := request.Params["cwd"].(string)
			result = map[string]any{"model": request.Params["model"], "modelProvider": "grok", "cwd": cwd, "approvalPolicy": "never", "approvalsReviewer": "user", "sandbox": map[string]any{"type": "readOnly"}, "thread": shippedProtocolThread(map[string]any{"id": "thread", "model": request.Params["model"], "modelProvider": "grok", "cwd": cwd})}
		case "turn/start":
			index := len(turns) + 1
			id := fmt.Sprintf("turn%d", index)
			schema, _ := request.Params["outputSchema"].(map[string]any)
			props, _ := schema["properties"].(map[string]any)
			if schema["type"] != "object" || schema["additionalProperties"] != false || props["summary"] == nil || (index == 1) != (props["articles"] != nil) {
				return
			}
			_, _ = io.WriteString(trace, "output_schema_verified\n")
			answer := `{"articles":[{"headline":"PRIVATE_HEADLINE","url":"https://news.example.test/story"}],"summary":null}`
			if index == 2 {
				answer = `{"summary":"PRIVATE_HEADLINE summary."}`
			}
			if mode == "schema_prose" {
				answer = "headline https://example.test"
			}
			if mode == "schema_followup_null" && index == 2 {
				answer = `{"summary":null}`
			}
			reply := map[string]any{"type": "agentMessage", "id": "reply", "phase": "final_answer", "text": answer}
			items := []any{reply}
			if mode == "split_final" {
				split := strings.Index(answer, ":") + 1
				reply["text"] = answer[:split]
				items = append(items, map[string]any{"type": "agentMessage", "id": "tail", "phase": "final_answer", "text": answer[split:]})
			}
			if mode == "commentary_after_final" {
				items = append(items, map[string]any{"type": "agentMessage", "id": "commentary", "phase": "commentary", "text": "Planning."})
			}
			if mode == "empty_reply" {
				items = []any{}
			}
			search := publicSearchFixtureCall()
			if mode == "unknown_public_type" {
				search["type"] = "unsupportedHostedSearch"
			}
			if index == 1 && mode != "missing" && mode != "raw_only" && mode != "started_only" && mode != "notification_only" && mode != "wrong_item_thread" && mode != "wrong_item_turn" {
				items = append(items, search)
			}
			if mode == "auxiliary_activity" {
				items = append(items, map[string]any{"type": "commandExecution", "id": "aux", "command": "true", "commandActions": []any{}, "cwd": ".", "status": "completed"})
			}
			terminal := map[string]any{"id": id, "status": "completed", "items": items}
			if mode == "failed_initial" || mode == "failed_followup" && index == 2 {
				terminal["status"] = "failed"
				terminal["error"] = map[string]any{"message": "PRIVATE_FAILURE", "codexErrorInfo": "other"}
			}
			turns = append(turns, terminal)
			started := map[string]any{"id": id, "status": "inProgress", "items": []any{}}
			if mode == "snapshot_without_terminal" {
				started["status"] = "completed"
				started["items"] = items
			}
			_ = output.Encode(map[string]any{"id": request.ID, "result": map[string]any{"turn": started}})
			notify := func(method, threadID, turnID string, item any) {
				p := map[string]any{"threadId": threadID, "turnId": turnID, "item": item}
				if method == "item/completed" {
					p["completedAtMs"] = 1
				}
				if method == "item/started" {
					p["startedAtMs"] = 1
				}
				_ = output.Encode(map[string]any{"method": method, "params": p})
			}
			if index == 1 {
				switch mode {
				case "missing", "settled_only", "failed_initial", "snapshot_without_terminal":
				case "raw_only":
					notify("rawResponseItem/completed", "thread", id, map[string]any{"type": "web_search_call", "id": "PRIVATE_RAW", "status": "completed", "action": map[string]any{"type": "search", "query": "PRIVATE_QUERY"}})
				case "started_only":
					notify("item/started", "thread", id, search)
				case "wrong_item_thread":
					notify("item/completed", "other", id, search)
				case "wrong_item_turn":
					notify("item/completed", "thread", "other", search)
				default:
					notify("item/completed", "thread", id, search)
					if mode == "duplicate" {
						notify("item/completed", "thread", id, search)
					}
				}
			}
			if mode == "no_terminal" || mode == "snapshot_without_terminal" {
				continue
			}
			threadID := "thread"
			completed := terminal
			if mode == "wrong_terminal_thread" {
				threadID = "other"
			}
			if mode == "wrong_terminal_turn" {
				completed = map[string]any{"id": "other", "status": "completed", "items": items}
			}
			_ = output.Encode(map[string]any{"method": "turn/completed", "params": map[string]any{"threadId": threadID, "turn": completed}})
			continue
		case "thread/read":
			history := append([]any(nil), turns...)
			if mode == "unrelated_history" {
				var unrelated []any
				for index := 0; index < 33; index++ {
					unrelated = append(unrelated, map[string]any{"id": fmt.Sprintf("unrelated-%d", index), "status": "failed", "items": []any{}})
				}
				history = append(unrelated, history...)
			}
			if mode == "reordered_history" && len(history) == 2 {
				history[0], history[1] = history[1], history[0]
			}
			if mode == "duplicate_current" {
				history = append(history, history[len(history)-1])
			}
			if mode == "missing_prior" && len(history) == 2 {
				history = history[1:]
			}
			if mode == "prior_failed" && len(history) == 2 {
				history[0].(map[string]any)["status"] = "failed"
			}
			id := "thread"
			if mode == "wrong_read" {
				id = "other"
			}
			result = map[string]any{"thread": shippedProtocolThread(map[string]any{"id": id, "turns": history})}
		default:
			return
		}
		_ = output.Encode(map[string]any{"id": request.ID, "result": result})
	}
}

func publicSearchFixtureCall() map[string]any {
	return map[string]any{"type": "webSearch", "id": "PRIVATE_ID", "query": "PRIVATE_QUERY", "action": map[string]any{"type": "search", "query": "PRIVATE_QUERY"}}
}

func TestHostedSearchPublicResultAndSingleContinuation(t *testing.T) {
	for _, scenario := range []string{"web", "web_allowed", "web_excluded"} {
		for _, mode := range []string{"ok", "duplicate", "settled_only", "notification_only", "split_final", "commentary_after_final", "reordered_history", "auxiliary_activity", "unrelated_history"} {
			t.Run(scenario+"/"+mode, func(t *testing.T) {
				t.Parallel()
				options := fixtureOptions(t, "search:"+mode+":"+scenario)
				ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
				defer cancel()
				got, err := live.HostedSearch(ctx, options, scenario)
				if err != nil || !got.Completed || !got.FirstCompleted || !got.FollowupCompleted || got.CanonicalCalls != 1 || got.SettledTurns != 2 || got.Turns != 2 {
					t.Fatalf("public hosted result: %+v %v", got, err)
				}
				assertSearchInvocationCount(t, options, 2)
				raw, _ := json.Marshal(got)
				if strings.Contains(string(raw), "PRIVATE_") {
					t.Fatal("private evidence leaked")
				}
			})
		}
	}
}

func TestHostedSearchRejectsMissingOrUnboundResult(t *testing.T) {
	for _, mode := range []string{"missing", "raw_only", "started_only", "unknown_public_type", "wrong_item_thread", "wrong_item_turn", "wrong_terminal_thread", "wrong_terminal_turn", "wrong_read", "no_terminal", "snapshot_without_terminal", "empty_reply", "failed_initial", "failed_followup", "duplicate_current", "missing_prior", "prior_failed"} {
		t.Run(mode, func(t *testing.T) {
			t.Parallel()
			options := fixtureOptions(t, "search:"+mode+":web")
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			got, err := live.HostedSearch(ctx, options, "web")
			if err == nil || got.Completed || got.FollowupCompleted || strings.Contains(err.Error(), "PRIVATE_") {
				t.Fatalf("unproven search accepted: %+v %v", got, err)
			}
			want := 1
			if mode == "failed_followup" || mode == "missing_prior" || mode == "prior_failed" {
				want = 2
			}
			assertSearchInvocationCount(t, options, want)
			if mode == "failed_initial" || mode == "failed_followup" {
				if live.DescribeFailure(err) != (live.FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "other"}) {
					t.Fatalf("lost terminal evidence: %v", err)
				}
			}
		})
	}
}

func assertSearchInvocationCount(t *testing.T, options live.Options, want int) {
	t.Helper()
	var script struct{ Trace string }
	_ = json.Unmarshal([]byte(options.APIKey), &script)
	trace, err := os.ReadFile(script.Trace)
	if err != nil || strings.Count(string(trace), "turn/start\n") != want {
		t.Fatalf("unexpected semantic invocation count: %q %v", trace, err)
	}
}

func TestHostedSearchRetiredXDoesNotLaunch(t *testing.T) {
	for _, scenario := range []string{"x", "x_window"} {
		t.Run(scenario, func(t *testing.T) {
			options := fixtureOptions(t, "search:ok:"+scenario)
			got, err := live.HostedSearch(context.Background(), options, scenario)
			if err == nil || live.DescribeFailure(err).Code != "hosted_x_retired" || got.Processes != 0 || got.Turns != 0 || got.Completed {
				t.Fatalf("retirement failed: %+v %v", got, err)
			}
		})
	}
}

func TestHostedSearchStructuredAnswersRejectWithoutRetry(t *testing.T) {
	for _, mode := range []string{"schema_prose", "schema_followup_null"} {
		t.Run(mode, func(t *testing.T) {
			options := fixtureOptions(t, "search:"+mode+":web")
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			got, err := live.HostedSearch(ctx, options, "web")
			if err == nil || got.Completed || (live.DescribeFailure(err).Code != "search_answer_invalid" && live.DescribeFailure(err).Code != "structured_reply_invalid") {
				t.Fatalf("invalid result accepted: %+v %v", got, err)
			}
			want := 1
			if mode == "schema_followup_null" {
				want = 2
			}
			assertSearchInvocationCount(t, options, want)
		})
	}
}
