package live_test

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/live"
)

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
	input, output := bufio.NewScanner(os.Stdin), json.NewEncoder(os.Stdout)
	var turns []any
	for input.Scan() {
		var request struct {
			ID     json.RawMessage `json:"id"`
			Method string          `json:"method"`
			Params map[string]any  `json:"params"`
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
			config := map[string]any{"web_search": "live"}
			switch scenario {
			case "web_allowed":
				config["tools.web_search.allowed_domains"] = []any{"reuters.com"}
			case "web_excluded":
				config["tools.web_search.excluded_domains"] = []any{"example.com"}
			case "x_window":
				config["model_providers.grok.x_search.from_date"] = "2026-08-01"
				config["model_providers.grok.x_search.to_date"] = "2026-08-15"
			}
			if !reflect.DeepEqual(request.Params["config"], config) || request.Params["experimentalRawEvents"] != true {
				return
			}
			result = map[string]any{"model": request.Params["model"], "modelProvider": "grok", "thread": map[string]any{"id": "thread", "modelProvider": "grok"}}
		case "turn/start":
			index := len(turns) + 1
			id := fmt.Sprintf("turn%d", index)
			if request.Params["threadId"] != "thread" {
				return
			}
			item := map[string]any{"type": "agentMessage", "id": "reply", "phase": "final_answer", "text": "PRIVATE_REPLY"}
			status := "completed"
			if mode == "failed" && index == 2 {
				status = "failed"
			}
			terminal := map[string]any{"id": id, "status": status, "items": []any{item}}
			if mode == "empty" {
				terminal["items"] = []any{}
			}
			turns = append(turns, terminal)
			_ = output.Encode(map[string]any{"id": request.ID, "result": map[string]any{"turn": map[string]any{"id": id, "status": "inProgress"}}})
			notify := func(method string, raw any) {
				thread, turnID := "thread", id
				if mode == "wrong_thread" {
					thread = "other"
				}
				if mode == "wrong_turn" {
					turnID = "other"
				}
				_ = output.Encode(map[string]any{"method": method, "params": map[string]any{"threadId": thread, "turnId": turnID, "item": raw}})
			}
			if mode == "started_local" {
				notify("item/started", map[string]any{"type": "dynamicToolCall", "id": "PRIVATE_CALL"})
			}
			if index == 2 && strings.HasPrefix(mode, "late_prior_") {
				if mode == "late_prior_local" {
					_ = output.Encode(map[string]any{"method": "rawResponseItem/completed", "params": map[string]any{"threadId": "thread", "turnId": "turn1", "item": map[string]any{"type": "function_call_output", "call_id": "PRIVATE_CALL"}}})
				} else {
					previous := turns[0]
					if mode == "late_prior_failed" {
						previous = map[string]any{"id": "turn1", "status": "failed"}
					}
					_ = output.Encode(map[string]any{"method": "turn/completed", "params": map[string]any{"threadId": "thread", "turn": previous}})
				}
			}
			if index == 1 && mode != "missing" {
				call := map[string]any{"type": "custom_tool_call", "id": "PRIVATE_ID", "status": "completed", "call_id": "PRIVATE_CALL", "name": "x_keyword_search", "input": "PRIVATE_INPUT"}
				if strings.HasPrefix(scenario, "web") {
					call = map[string]any{"type": "web_search_call", "id": "PRIVATE_ID", "status": "completed", "action": map[string]any{"type": "search", "query": "PRIVATE_QUERY"}}
				}
				if mode == "action_only" && strings.HasPrefix(scenario, "web") {
					call["action"] = map[string]any{"type": "search"}
				}
				if mode == "unsupported" {
					call["name"] = "x_unknown"
				}
				if mode == "partial" {
					call["status"] = "in_progress"
				}
				if mode == "output_first" {
					notify("rawResponseItem/completed", map[string]any{"type": "custom_tool_call_output", "call_id": "PRIVATE_CALL", "output": "PRIVATE_CANARY"})
				}
				notify("rawResponseItem/completed", call)
				if mode == "duplicate" {
					notify("rawResponseItem/completed", call)
				}
				if mode == "conflict" {
					call["input"] = "changed"
					notify("rawResponseItem/completed", call)
				}
				if mode == "local" {
					notify("item/completed", map[string]any{"type": "dynamicToolCall", "id": "PRIVATE_CALL"})
				}
			}
			if mode == "no_terminal" {
				return
			}
			_ = output.Encode(map[string]any{"method": "turn/completed", "params": map[string]any{"threadId": "thread", "turn": terminal}})
			continue
		case "thread/read":
			id := "thread"
			if mode == "wrong_read" {
				id = "other"
			}
			localKind := map[string]string{
				"settled_dynamic": "dynamicToolCall", "settled_mcp": "mcpToolCall", "settled_command": "commandExecution",
				"settled_function_output": "functionCallOutput", "settled_file_change": "fileChange",
				"settled_image_view": "imageView", "settled_image_generation": "imageGeneration",
				"settled_collab": "collabAgentToolCall", "settled_child": "subAgentActivity", "settled_sleep": "sleep",
				"replay_settled_dynamic": "dynamicToolCall", "replay_settled_mcp": "mcpToolCall", "replay_settled_command": "commandExecution",
			}[mode]
			if localKind != "" && (len(turns) == 1 && strings.HasPrefix(mode, "settled_") || len(turns) == 2 && strings.HasPrefix(mode, "replay_settled_")) {
				current := turns[len(turns)-1].(map[string]any)
				current["items"] = append(current["items"].([]any), map[string]any{"type": localKind, "id": "PRIVATE_CALL"})
			}
			if len(turns) == 2 {
				previous := turns[0].(map[string]any)
				switch mode {
				case "prior_failed":
					previous["status"] = "failed"
				case "prior_error":
					previous["error"] = map[string]any{"message": "PRIVATE_CANARY"}
				case "prior_local":
					previous["items"] = append(previous["items"].([]any), map[string]any{"type": "dynamicToolCall", "id": "PRIVATE_CALL"})
				}
			}
			result = map[string]any{"thread": map[string]any{"id": id, "turns": turns}}
		default:
			return
		}
		_ = output.Encode(map[string]any{"id": request.ID, "result": result})
	}
}

func TestHostedSearchRetainsEachScenarioAndSingleContinuation(t *testing.T) {
	for _, scenario := range []string{"web", "web_allowed", "web_excluded", "x", "x_window"} {
		for _, mode := range []string{"ok", "duplicate", "action_only"} {
			t.Run(scenario+"/"+mode, func(t *testing.T) {
				options := fixtureOptions(t, "search:"+mode+":"+scenario)
				ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
				defer cancel()
				got, err := live.HostedSearch(ctx, options, scenario)
				if err != nil || !got.Completed || !got.FirstCompleted || !got.FollowupCompleted || got.CanonicalCalls != 1 || got.SettledTurns != 2 || got.Turns != 2 || got.Processes != 1 || got.Initializations != 1 || got.Threads != 1 {
					t.Fatalf("hosted result: %+v, %v", got, err)
				}
				var script struct{ Trace string }
				_ = json.Unmarshal([]byte(options.APIKey), &script)
				trace, _ := os.ReadFile(script.Trace)
				if string(trace) != "initialize\ninitialized\nthread/start\nturn/start\nthread/read\nturn/start\nthread/read\n" {
					t.Fatalf("unexpected semantic invocation trace: %q", trace)
				}
				encoded, _ := json.Marshal(got)
				if strings.Contains(string(encoded), "PRIVATE_") {
					t.Fatal("private evidence leaked")
				}
			})
		}
	}
}

func TestHostedSearchRejectsMissingConflictAndLocalExecution(t *testing.T) {
	for _, mode := range []string{"missing", "unsupported", "partial", "output_first", "conflict", "local", "wrong_thread", "wrong_turn", "wrong_read", "no_terminal", "empty", "failed"} {
		t.Run(mode, func(t *testing.T) {
			options := fixtureOptions(t, "search:"+mode+":x")
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			got, err := live.HostedSearch(ctx, options, "x")
			if err == nil || got.Completed || got.FollowupCompleted || strings.Contains(err.Error(), "PRIVATE_") {
				t.Fatalf("invalid evidence accepted or exposed: %+v, %v", got, err)
			}
			want := 1
			if mode == "failed" {
				want = 2
			}
			var script struct{ Trace string }
			_ = json.Unmarshal([]byte(options.APIKey), &script)
			trace, _ := os.ReadFile(script.Trace)
			if strings.Count(string(trace), "turn/start\n") != want || got.Turns != want {
				t.Fatalf("failed invocation resubmitted: %+v", got)
			}
		})
	}
}

func TestHostedSearchRejectsContradictorySettledEvidence(t *testing.T) {
	for _, mode := range []string{
		"settled_dynamic", "settled_mcp", "settled_command", "settled_function_output", "settled_file_change",
		"settled_image_view", "settled_image_generation", "settled_collab", "settled_child", "settled_sleep",
		"replay_settled_dynamic", "replay_settled_mcp", "replay_settled_command", "prior_failed", "prior_error", "prior_local",
		"late_prior_local", "late_prior_failed", "started_local",
	} {
		t.Run(mode, func(t *testing.T) {
			options := fixtureOptions(t, "search:"+mode+":x")
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			got, err := live.HostedSearch(ctx, options, "x")
			wantTurns, wantSettled := 1, 0
			if !strings.HasPrefix(mode, "settled_") && mode != "started_local" {
				wantTurns, wantSettled = 2, 1
			}
			wantError := "live: local execution cannot establish hosted search"
			if mode == "prior_failed" || mode == "prior_error" || mode == "late_prior_failed" {
				wantError = "live: search turn did not complete"
			}
			if err == nil || err.Error() != wantError || got.Completed || got.FollowupCompleted || got.Turns != wantTurns || got.SettledTurns != wantSettled || got.FirstCompleted != (wantTurns == 2) || got.CanonicalCalls != 1 && mode != "started_local" {
				t.Fatalf("contradictory settled evidence accepted or prior evidence lost: %+v, %v", got, err)
			}
			var script struct{ Trace string }
			_ = json.Unmarshal([]byte(options.APIKey), &script)
			trace, _ := os.ReadFile(script.Trace)
			if strings.Count(string(trace), "turn/start\n") != wantTurns {
				t.Fatal("contradictory history caused resubmission")
			}
		})
	}
}

func TestHostedSearchAcceptsConsistentDelayedPriorTerminal(t *testing.T) {
	options := fixtureOptions(t, "search:late_prior_completed:x")
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	got, err := live.HostedSearch(ctx, options, "x")
	if err != nil || !got.Completed || got.Turns != 2 || got.CanonicalCalls != 1 || got.SettledTurns != 2 {
		t.Fatalf("consistent delayed prior terminal rejected: %+v, %v", got, err)
	}
}
