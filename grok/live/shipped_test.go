package live_test

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/dist"
	"github.com/Harness-X-Harness/codex/grok/live"
)

func fixtureShippedCatalog() map[string]any {
	rows := []any{}
	for index, slug := range []string{"grok-4.7", "grok-4.6"} {
		efforts := []any{}
		for _, pair := range [][2]string{{"ultra", "Ultra reasoning"}, {"xhigh", "Maximum reasoning"}, {"high", "High reasoning"}, {"medium", "Medium reasoning"}, {"low", "Low reasoning"}} {
			efforts = append(efforts, map[string]any{"reasoningEffort": pair[0], "description": pair[1]})
		}
		name := "Grok " + strings.TrimPrefix(slug, "grok-")
		rows = append(rows, map[string]any{"id": slug, "model": slug, "displayName": name, "description": name + " model", "upgrade": nil, "upgradeInfo": nil, "availabilityNux": nil, "modelSpecialty": nil, "hidden": false, "supportedReasoningEfforts": efforts, "defaultReasoningEffort": "high", "inputModalities": []any{"text", "image"}, "supportsPersonality": false, "multiAgentVersion": "v2", "additionalSpeedTiers": []any{}, "serviceTiers": []any{}, "defaultServiceTier": nil, "availableAccessPrograms": nil, "isDefault": index == 0})
	}
	return map[string]any{"data": rows, "nextCursor": nil}
}

func shippedProtocolThread(thread map[string]any) map[string]any {
	if thread == nil {
		thread = map[string]any{}
	}
	cwd, err := os.Getwd()
	if err != nil {
		cwd = os.Getenv("CODEX_HOME")
	}
	defaults := map[string]any{
		"cliVersion": "shipped-fixture", "createdAt": 1, "updatedAt": 1, "cwd": cwd,
		"ephemeral": false, "preview": "", "projectId": nil, "sessionId": "shipped-session",
		"status": map[string]any{"type": "idle"}, "source": "appServer", "model": "grok-4.7", "modelProvider": "grok",
	}
	for key, value := range defaults {
		if _, ok := thread[key]; !ok || thread[key] == nil && key != "projectId" {
			thread[key] = value
		}
	}
	if thread["turns"] == nil {
		thread["turns"] = []any{}
	}
	if turns, ok := thread["turns"].([]any); ok {
		for _, raw := range turns {
			turn, ok := raw.(map[string]any)
			if !ok {
				continue
			}
			if turn["items"] == nil {
				turn["items"] = []any{}
			}
			if _, ok := turn["status"]; !ok {
				turn["status"] = "completed"
			}
		}
	}
	if source, ok := thread["source"].(map[string]any); ok {
		if sub, ok := source["subAgent"].(map[string]any); ok {
			if spawn, ok := sub["thread_spawn"].(map[string]any); ok {
				if _, ok := spawn["depth"]; !ok {
					spawn["depth"] = 1
				}
				if _, ok := spawn["parent_thread_id"]; !ok {
					spawn["parent_thread_id"] = "PRIVATE_PARENT"
				}
			}
		}
	}
	return thread
}

func copiedShippedAssets() bool {
	profile, err := os.ReadFile(filepath.Join(os.Getenv("CODEX_HOME"), "config.toml"))
	if err != nil {
		return false
	}
	catalog, err := os.ReadFile(filepath.Join(os.Getenv("CODEX_HOME"), "models.json"))
	if err != nil {
		return false
	}
	cwd, err := os.Getwd()
	return err == nil && filepath.Dir(cwd) == os.Getenv("CODEX_HOME") && string(profile) == dist.Profile() && bytes.Equal(catalog, dist.Catalog())
}

const shippedInvoice = "item,quantity,unit_price,line_total\nnotebook,3,4.50,13.50\npen,2,1.25,2.50\ntotal,,,16.00"

func fakeShippedServer() {
	var script struct{ Mode, Trace string }
	if json.Unmarshal([]byte(os.Getenv("GROK_API_KEY")), &script) != nil || !copiedShippedAssets() {
		return
	}
	trace, err := os.OpenFile(script.Trace, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
	if err != nil {
		return
	}
	defer trace.Close()
	signal.Notify(make(chan os.Signal, 1), os.Interrupt)
	mode := strings.TrimPrefix(script.Mode, "shipped:")
	childPath := "/root/checker"
	if mode == "long_child_path" {
		childPath = "/root/" + strings.Repeat("checker", 180)
	}
	input, output := bufio.NewScanner(os.Stdin), json.NewEncoder(os.Stdout)
	message := func(id, text string) map[string]any {
		return map[string]any{"type": "agentMessage", "id": id, "text": text}
	}
	turn := func(id, text string) map[string]any {
		return map[string]any{"id": id, "status": "completed", "error": nil, "items": []any{message(id+"-final", text)}}
	}
	parentTurn := turn("PRIVATE_PARENT_TURN", shippedInvoice)
	childTurn := turn("PRIVATE_CHILD_TURN", shippedInvoice)
	if mode == "parent_wrong_invoice" {
		parentTurn = turn("PRIVATE_PARENT_TURN", strings.Replace(shippedInvoice, "16.00", "15.00", 1))
	}
	if mode == "child_wrong_invoice" {
		childTurn = turn("PRIVATE_CHILD_TURN", strings.Replace(shippedInvoice, "13.50", "12.50", 1))
	}
	if mode == "child_without_invoice" || mode == "ready_late_child_failure" {
		childTurn = turn("PRIVATE_CHILD_TURN", "The invoice is ready.")
	}
	if mode == "child_failed" {
		childTurn["status"] = "failed"
	}
	if mode == "child_tool_rounds" {
		childTurn["items"] = []any{message("child-interim", "Checking the order."), map[string]any{"type": "functionCallOutput", "id": "check-output", "name": "check", "output": "done"}, message("PRIVATE_CHILD_TURN-final", shippedInvoice)}
	}
	if mode == "parent_tool_rounds" {
		parentTurn["items"] = []any{message("interim", "Checking the order."), map[string]any{"type": "functionCallOutput", "id": "check-output", "name": "check", "output": "done"}, message("PRIVATE_PARENT_TURN-final", shippedInvoice)}
	}
	parentItems := parentTurn["items"].([]any)
	activity := map[string]any{"type": "subAgentActivity", "id": "spawn-call", "kind": "started", "agentThreadId": "PRIVATE_CHILD", "agentPath": childPath}
	if mode == "completed_activity" {
		activity["kind"] = "completed"
	}
	if mode == "collab_receiver" {
		activity = map[string]any{"type": "collabAgentToolCall", "id": "spawn-call", "tool": "spawnAgent", "status": "completed", "senderThreadId": "PRIVATE_PARENT", "receiverThreadIds": []any{"PRIVATE_CHILD"}, "agentsStates": map[string]any{}}
	}
	if mode != "parent_only_invoice" && mode != "early_spawn_failure" && mode != "terminal_hint_only" {
		parentTurn["items"] = append([]any{activity}, parentItems...)
	}
	notify := func(method, thread, tid string, item any) {
		params := map[string]any{"threadId": thread, "turnId": tid}
		if method == "rawResponseItem/completed" {
			params["item"] = item
		} else if method == "thread/closed" {
		} else {
			params["turn"] = item
		}
		_ = output.Encode(map[string]any{"method": method, "params": params})
	}
	rawFinal := func() {
		text := shippedInvoice
		if mode == "raw_text_not_authority" {
			text = "hidden markup, not the durable business answer"
		}
		notify("rawResponseItem/completed", "PRIVATE_PARENT", "PRIVATE_PARENT_TURN", map[string]any{"type": "message", "id": "PRIVATE_PARENT_TURN-final", "role": "assistant", "content": []any{map[string]any{"type": "output_text", "text": text}}})
	}
	delivery := func() {
		if mode == "missing_delivery" {
			rawFinal()
			return
		}
		author, recipient, text := childPath, "/root", shippedInvoice
		if mode == "wrong_delivery_author" {
			author = "/root/other"
		}
		if mode == "wrong_delivery_recipient" {
			recipient = "/root/other"
		}
		if mode == "wrong_delivery_invoice" {
			text = "ready"
		}
		if mode == "pre_delivery_replay" {
			rawFinal()
		}
		notify("rawResponseItem/completed", "PRIVATE_PARENT", "PRIVATE_PARENT_TURN", map[string]any{"type": "agent_message", "author": author, "recipient": recipient, "content": []any{map[string]any{"type": "input_text", "text": "Message Type: FINAL_ANSWER\nTask name: /root\nSender: " + author + "\nPayload:\n" + text}}})
		rawFinal()
	}
	children := func() {
		if mode == "parent_only_invoice" || mode == "early_spawn_failure" {
			if mode == "early_spawn_failure" {
				// This is a controlled pre-Started soft-failure trace, not a
				// claim about why a real model/runtime omitted child activity.
				notify("rawResponseItem/completed", "PRIVATE_PARENT", "PRIVATE_PARENT_TURN", map[string]any{"type": "function_call", "id": "prestarted-item", "call_id": "prestarted-call", "name": "spawn_agent", "arguments": `{"message":"PRIVATE_DELEGATED_TASK","fork_turns":"PRIVATE_INVALID_MODE"}`})
				notify("rawResponseItem/completed", "PRIVATE_PARENT", "PRIVATE_PARENT_TURN", map[string]any{"type": "function_call_output", "call_id": "prestarted-call", "output": `{"success":false,"error":"PRIVATE_SPAWN_ERROR"}`})
			}
			rawFinal()
			return
		}
		notify("turn/completed", "PRIVATE_CHILD", "PRIVATE_CHILD_TURN", childTurn)
		if mode == "optional_raw_noise" {
			for i := 0; i < 16; i++ {
				notify("rawResponseItem/completed", "PRIVATE_PARENT", "PRIVATE_PARENT_TURN", map[string]any{"type": "function_call", "id": fmt.Sprintf("noise-%d", i), "call_id": fmt.Sprintf("noise-%d", i), "name": "diagnostic_only", "arguments": "{}", "status": "completed"})
			}
		}
		if mode != "delayed_delivery" {
			delivery()
		}
	}
	starts := 0
	for input.Scan() {
		var req struct {
			ID     json.RawMessage
			Method string
			Params map[string]any
		}
		if json.Unmarshal(input.Bytes(), &req) != nil {
			return
		}
		_, _ = io.WriteString(trace, req.Method+"\n")
		var result any
		switch req.Method {
		case "initialize":
			family := "unix"
			if runtime.GOOS == "windows" {
				family = "windows"
			}
			result = map[string]any{"userAgent": "fixture", "codexHome": os.Getenv("CODEX_HOME"), "platformFamily": family, "platformOs": runtime.GOOS}
		case "initialized":
			continue
		case "model/list":
			catalog := fixtureShippedCatalog()
			rows := catalog["data"].([]any)
			switch mode {
			case "extra_model":
				catalog["data"] = append(rows, rows[0])
			case "missing_model":
				catalog["data"] = rows[:1]
			case "reordered":
				catalog["data"] = []any{rows[1], rows[0]}
			case "duplicate_model":
				catalog["data"] = []any{rows[0], rows[0]}
			case "wrong_default":
				rows[1].(map[string]any)["isDefault"] = true
			case "missing_effort":
				rows[0].(map[string]any)["supportedReasoningEfforts"] = []any{}
			case "wrong_effort":
				rows[0].(map[string]any)["defaultReasoningEffort"] = "low"
			case "missing_v2":
				delete(rows[0].(map[string]any), "multiAgentVersion")
			case "missing_modalities":
				rows[0].(map[string]any)["inputModalities"] = []any{"text"}
			case "pagination":
				catalog["nextCursor"] = "more"
			}
			result = catalog
		case "thread/start":
			if req.Params["model"] != "grok-4.7" || req.Params["experimentalRawEvents"] != true {
				return
			}
			cwd, _ := os.Getwd()
			provider := "grok"
			if mode == "wrong_start_provider" {
				provider = "PRIVATE_OTHER"
			}
			result = map[string]any{"model": "grok-4.7", "modelProvider": provider, "cwd": cwd, "approvalPolicy": "never", "approvalsReviewer": "user", "sandbox": map[string]any{"type": "readOnly"}, "thread": shippedProtocolThread(map[string]any{"id": "PRIVATE_PARENT"})}
		case "turn/start":
			starts++
			if starts > 1 || req.Params["threadId"] != "PRIVATE_PARENT" {
				return
			}
			if req.Params["effort"] != nil || req.Params["outputSchema"] != nil {
				return
			}
			if !strings.HasPrefix(mode, "startup") {
				parts, ok := req.Params["input"].([]any)
				if !ok || len(parts) != 1 {
					return
				}
				prompt, _ := parts[0].(map[string]any)["text"].(string)
				if !strings.Contains(prompt, "subagent") || !strings.Contains(prompt, "3 notebooks") || !strings.Contains(prompt, "CSV invoice") || strings.Contains(prompt, "nonce") || strings.Contains(prompt, "fork") {
					return
				}
				_, _ = io.WriteString(trace, "task/start\n")
				children()
			} else {
				parentTurn = turn("PRIVATE_PARENT_TURN", "Ready.")
			}
			result = map[string]any{"turn": map[string]any{"id": "PRIVATE_PARENT_TURN", "status": "inProgress", "items": []any{}}}
		case "thread/read":
			id, _ := req.Params["threadId"].(string)
			thread := map[string]any{"id": id, "turns": []any{parentTurn}}
			if id != "PRIVATE_PARENT" {
				thread = map[string]any{"id": id, "parentThreadId": "PRIVATE_PARENT", "forkedFromId": "PRIVATE_PARENT", "source": map[string]any{"subAgent": map[string]any{"thread_spawn": map[string]any{"agent_path": childPath, "parent_thread_id": "PRIVATE_PARENT", "depth": 1}}}, "turns": []any{childTurn}}
				switch mode {
				case "child_parent":
					thread["parentThreadId"] = "PRIVATE_OTHER"
				case "child_provider":
					thread["modelProvider"] = "PRIVATE_OTHER"
				case "child_model":
					thread["model"] = "PRIVATE_OTHER"
				case "child_pinned_model":
					thread["model"] = "grok-4.6"
				case "child_fresh_fork":
					thread["forkedFromId"] = nil
				case "child_unit_source":
					thread["source"] = "appServer"
				case "child_self_path":
					thread["source"] = map[string]any{"subAgent": map[string]any{"thread_spawn": map[string]any{"agent_path": "/root", "parent_thread_id": "PRIVATE_PARENT", "depth": 1}}}
				}
				if mode == "late_child_failure" || mode == "ready_late_child_failure" {
					notify("turn/completed", "PRIVATE_CHILD", "PRIVATE_CHILD_TURN", map[string]any{"id": "PRIVATE_CHILD_TURN", "status": "failed", "error": nil, "items": []any{}})
				}
			}
			if strings.HasPrefix(mode, "startup_late_") {
				tid, status := "PRIVATE_PARENT", "failed"
				var terr any
				if mode == "startup_late_error" {
					status = "completed"
					terr = map[string]any{"message": "PRIVATE_ERROR"}
				}
				if mode == "startup_late_unrelated" {
					tid = "PRIVATE_OTHER"
				}
				notify("turn/completed", tid, "PRIVATE_PARENT_TURN", map[string]any{"id": "PRIVATE_PARENT_TURN", "status": status, "error": terr, "items": []any{}})
			}
			if mode == "long_history" && req.Params["threadId"] == "PRIVATE_PARENT" {
				turns := thread["turns"].([]any)
				for i := 0; i < 33; i++ {
					turns = append([]any{turn(fmt.Sprintf("prior-%d", i), "Earlier task.")}, turns...)
				}
				thread["turns"] = turns
			}
			result = map[string]any{"thread": shippedProtocolThread(thread)}
		default:
			return
		}
		_ = output.Encode(map[string]any{"id": req.ID, "result": result})
		if req.Method == "thread/read" && req.Params["threadId"] == "PRIVATE_CHILD" && mode == "delayed_delivery" {
			delivery()
		}
		if req.Method == "turn/start" {
			status := "completed"
			if mode == "failed" {
				status = "failed"
			}
			notify("turn/completed", "PRIVATE_PARENT", "PRIVATE_PARENT_TURN", map[string]any{"id": "PRIVATE_PARENT_TURN", "status": status, "error": nil, "items": []any{}})
		}
	}
}

func TestShippedCatalogRequiresCompleteDTOs(t *testing.T) {
	for _, mode := range []string{"ok", "extra_model", "missing_model", "duplicate_model", "reordered", "wrong_default", "missing_effort", "wrong_effort", "missing_v2", "missing_modalities", "pagination"} {
		t.Run(mode, func(t *testing.T) {
			opts := fixtureOptions(t, "shipped:"+mode)
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			got, err := live.ShippedCatalog(ctx, opts.Subject, opts.APIKey)
			if (err == nil) != (mode == "ok") {
				t.Fatalf("catalog oracle: %+v %v", got, err)
			}
			if err == nil && (!got.ShippedCatalog || got.CatalogModels != 2 || got.Turns != 0 || got.Threads != 0) {
				t.Fatal("catalog-only scope changed")
			}
		})
	}
}

func TestShippedStartupAndChildRejectIncompleteEvidence(t *testing.T) {
	success := map[string]bool{"ok": true, "long_child_path": true, "long_history": true, "completed_activity": true, "terminal_hint_only": true, "collab_receiver": true, "child_pinned_model": true, "child_fresh_fork": true, "raw_text_not_authority": true, "delayed_delivery": true, "optional_raw_noise": true, "parent_tool_rounds": true, "child_tool_rounds": true}
	modes := []string{"ok", "long_child_path", "long_history", "completed_activity", "terminal_hint_only", "collab_receiver", "child_pinned_model", "child_fresh_fork", "raw_text_not_authority", "delayed_delivery", "optional_raw_noise", "parent_tool_rounds", "child_tool_rounds", "parent_only_invoice", "early_spawn_failure", "child_without_invoice", "child_wrong_invoice", "child_failed", "child_parent", "child_provider", "child_model", "child_unit_source", "child_self_path", "missing_delivery", "wrong_delivery_author", "wrong_delivery_recipient", "wrong_delivery_invoice", "pre_delivery_replay", "parent_wrong_invoice", "late_child_failure", "ready_late_child_failure", "wrong_start_provider", "failed"}
	for _, mode := range modes {
		t.Run(mode, func(t *testing.T) {
			opts := fixtureOptions(t, "shipped:"+mode)
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			got, err := live.ShippedChildCollaboration(ctx, opts.Subject, opts.APIKey)
			if (err == nil) != success[mode] {
				t.Fatalf("child oracle: %+v %v", got, err)
			}
			if err == nil && (!got.ChildBound || !got.ChildCompleted || !got.ChildResultDelivered || got.Turns != 1 || got.SetupTurns != 0 || got.SetupCompleted || got.TaskTurns != 1) {
				t.Fatal("complete child invoice proof absent")
			}
			if mode == "child_without_invoice" && (!got.ChildBound || !got.ChildCompleted || live.DescribeFailure(err).Code != "child_invoice_not_observed") {
				t.Fatalf("truthful partial child facts lost: %+v %v", got, err)
			}
			if (mode == "late_child_failure" || mode == "ready_late_child_failure") && got.ChildCompleted {
				t.Fatal("admitted terminal contradiction preserved completion")
			}
			if mode == "parent_only_invoice" && live.DescribeFailure(err).Code != "child_binding_not_observed" {
				t.Fatal("parent-only invoice credited")
			}
			if strings.Contains(fmt.Sprint(got, err), "PRIVATE_") {
				t.Fatal("private fixture data escaped")
			}
			var script struct{ Trace string }
			_ = json.Unmarshal([]byte(opts.APIKey), &script)
			trace, _ := os.ReadFile(script.Trace)
			if strings.Count(string(trace), "turn/start\n") > 1 || strings.Count(string(trace), "task/start\n") > 1 {
				t.Fatal("task resubmitted")
			}
		})
	}
	opts := fixtureOptions(t, "shipped:startup")
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	got, err := live.ShippedStartup(ctx, opts.Subject, opts.APIKey)
	if err != nil || !got.Completed || !got.ShippedCatalog || got.Turns != 1 {
		t.Fatalf("startup: %+v %v", got, err)
	}
}

func TestShippedPinnedUsesRetainedHistoryScenario(t *testing.T) {
	opts := fixtureOptions(t, "history:shipped")
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	got, err := live.ShippedPinned(ctx, opts.Subject, opts.APIKey)
	if err != nil || got.Model != "grok-4.6" || !got.ShippedCatalog || !got.Recalled || got.Turns != 2 {
		t.Fatalf("pinned: %+v %v", got, err)
	}
}

func TestShippedStartupRevalidatesAdmittedTerminal(t *testing.T) {
	for _, mode := range []string{"startup_late_failed", "startup_late_error", "startup_late_unrelated"} {
		t.Run(mode, func(t *testing.T) {
			opts := fixtureOptions(t, "shipped:"+mode)
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			got, err := live.ShippedStartup(ctx, opts.Subject, opts.APIKey)
			accepted := mode == "startup_late_unrelated"
			if (err == nil) != accepted || got.Completed != accepted || got.Turns != 1 {
				t.Fatalf("terminal contradiction: %+v %v", got, err)
			}
			if strings.Contains(fmt.Sprint(got, err), "PRIVATE_") {
				t.Fatal("private data escaped")
			}
		})
	}
}

func TestChildInvoiceDiagnosticNoninterference(t *testing.T) {
	for _, mode := range []string{"ok", "child_without_invoice", "parent_only_invoice", "early_spawn_failure"} {
		for _, sink := range []string{"enabled", "closed", "unwritable"} {
			t.Run(mode+"/"+sink, func(t *testing.T) {
				opts := fixtureOptions(t, "shipped:"+mode)
				dir := t.TempDir()
				if sink == "unwritable" {
					dir = filepath.Join(dir, "not-a-directory")
					if err := os.WriteFile(dir, []byte("x"), 0600); err != nil {
						t.Fatal(err)
					}
				}
				diagnostics := live.NewDiagnostics(dir)
				if sink == "closed" {
					diagnostics.Close()
				}
				ctx, cancel := context.WithTimeout(context.Background(), time.Second)
				defer cancel()
				got, err := live.ShippedChildCollaboration(ctx, opts.Subject, opts.APIKey, diagnostics)
				diagnostics.Close()
				if (err == nil) != (mode == "ok") {
					t.Fatalf("optional sink changed outcome: %+v %v", got, err)
				}
				if mode == "early_spawn_failure" && (live.DescribeFailure(err).Code != "child_binding_not_observed" || got.ChildBound || got.ChildCompleted || got.ChildResultDelivered) {
					t.Fatalf("diagnostic call credited child work: %+v %v", got, err)
				}
				if sink == "enabled" {
					data, readErr := os.ReadFile(filepath.Join(dir, "events.jsonl"))
					if readErr != nil {
						t.Fatal(readErr)
					}
					if !strings.Contains(string(data), "child_parent_result") || !strings.Contains(string(data), "invoice_correct") {
						t.Fatal("product diagnostic facts absent")
					}
					if mode == "early_spawn_failure" {
						for _, want := range []string{"response_tool_shape", "prestarted-call", `"fork_turns":"string"`, `"success":false`, `"error_present":true`} {
							if !strings.Contains(string(data), want) {
								t.Errorf("pre-Started diagnostic fact missing: %s", want)
							}
						}
						for _, private := range []string{"PRIVATE_DELEGATED_TASK", "PRIVATE_INVALID_MODE", "PRIVATE_SPAWN_ERROR"} {
							if strings.Contains(string(data), private) {
								t.Errorf("optional raw-tool projection retained payload: %s", private)
							}
						}
					}
				}
			})
		}
	}
}
