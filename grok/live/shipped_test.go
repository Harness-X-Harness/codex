package live_test

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
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
	mode := strings.TrimPrefix(script.Mode, "shipped:")
	input, output := bufio.NewScanner(os.Stdin), json.NewEncoder(os.Stdout)
	nonce, other := "12345678-1234-4234-8234-123456789abc", "87654321-1234-4234-8234-123456789abc"
	agent := func(id, text string) map[string]any {
		return map[string]any{"type": "agentMessage", "id": id, "phase": "final_answer", "text": text}
	}
	user := func(id, text string) map[string]any {
		return map[string]any{"type": "userMessage", "id": id, "content": []any{map[string]any{"type": "text", "text": text, "textElements": []any{}}}}
	}
	clone := func(value map[string]any) map[string]any {
		raw, _ := json.Marshal(value)
		var copied map[string]any
		_ = json.Unmarshal(raw, &copied)
		return copied
	}
	var seed map[string]any
	taskPrompt := ""
	phase := 0
	snapshot := func(id string) any {
		if phase == 1 && mode != "startup" {
			return map[string]any{"thread": map[string]any{"id": "PRIVATE_PARENT", "model": "grok-4.7", "modelProvider": "grok", "turns": []any{seed}}}
		}
		spawn := map[string]any{"type": "subAgentActivity", "id": "spawn", "kind": "started", "agentThreadId": "PRIVATE_CHILD", "agentPath": "/root/live_child"}
		reply := agent("reply", nonce)
		if mode == "parent_prose" {
			reply["text"] = "The delegated result is " + nonce + "."
		}
		if mode == "ambiguous_results" || mode == "multiple_results" {
			reply["text"] = nonce + " and " + other
		}
		current := map[string]any{"id": "PRIVATE_PARENT_TURN", "status": "completed", "error": nil, "items": []any{user("task-input", taskPrompt), spawn, reply}}
		childTurn := map[string]any{"id": "PRIVATE_CHILD_TURN", "status": "completed", "error": nil, "items": []any{agent("child-reply", nonce)}}
		if mode == "split_after_result_mention" || mode == "split_before_result_supplied" {
			childTurn["items"] = []any{agent("child-reply", nonce), agent("child-tail", "That is the result.")}
		}
		parentTurns := []any{current}
		childTurns := []any{childTurn}
		if seed != nil {
			parentTurns = []any{clone(seed), current}
			childTurns = []any{clone(seed), childTurn}
		}
		parent := map[string]any{"id": "PRIVATE_PARENT", "model": "grok-4.7", "modelProvider": "grok", "turns": parentTurns}
		child := map[string]any{"id": "PRIVATE_CHILD", "parentThreadId": "PRIVATE_PARENT", "forkedFromId": "PRIVATE_PARENT", "source": map[string]any{"subAgent": map[string]any{"thread_spawn": map[string]any{"agent_path": "/root/live_child"}}}, "model": "grok-4.7", "modelProvider": "grok", "turns": childTurns}
		switch mode {
		case "stale":
			parent["turns"] = []any{seed}
		case "duplicate_turn":
			parent["turns"] = append(parentTurns, current)
		case "missing_reply":
			current["items"] = []any{spawn}
		case "wrong_history_provider":
			parent["modelProvider"] = "PRIVATE_OTHER"
		case "wrong_history_model":
			parent["model"] = "PRIVATE_OTHER"
		case "parent_history_changed":
			parentTurns[0].(map[string]any)["id"] = "PRIVATE_OTHER_SEED"
		case "child_provider":
			child["modelProvider"] = "PRIVATE_OTHER"
		case "child_model":
			child["model"] = "PRIVATE_OTHER"
		case "child_parent":
			child["parentThreadId"] = "PRIVATE_OTHER"
		case "child_fresh":
			child["forkedFromId"] = nil
		case "child_failed":
			childTurn["status"] = "failed"
		case "child_interrupted":
			childTurn["status"] = "interrupted"
		case "child_missing":
			child["turns"] = []any{}
		case "child_inherited":
			childTurn["id"] = "PRIVATE_PARENT_TURN"
		case "child_mismatch":
			childTurn["items"] = []any{agent("child-reply", other)}
		case "child_no_history":
			child["turns"] = []any{childTurn}
		case "child_partial_history":
			child["turns"] = []any{map[string]any{"id": "PRIVATE_PARENT_TURN", "status": "interrupted", "items": []any{user("task-input", taskPrompt)}}, childTurn}
		case "inherited_supplied", "inherited_task_history", "inherited_diagnostic":
			text := "delegating the task"
			if mode == "inherited_supplied" {
				text = "Use " + nonce
			}
			child["turns"] = []any{clone(seed), map[string]any{"id": "PRIVATE_PARENT_TURN", "status": "interrupted", "items": []any{user("task-input", taskPrompt), agent("parent-before-spawn", text)}}, childTurn}
			if mode == "inherited_diagnostic" {
				child["turns"].([]any)[1].(map[string]any)["items"] = []any{map[string]any{"type": "agentMessage", "id": nonce, "phase": "commentary", "text": "diagnostic " + nonce}}
			}
		case "child_history_id":
			childTurns[0].(map[string]any)["id"] = "PRIVATE_OTHER_SEED"
		case "child_history_item_id":
			childTurns[0].(map[string]any)["items"].([]any)[0].(map[string]any)["id"] = "PRIVATE_OTHER_INPUT"
		case "child_history_content":
			childTurns[0].(map[string]any)["items"].([]any)[0] = user("seed-input", "altered seed")
		case "child_history_order":
			items := childTurns[0].(map[string]any)["items"].([]any)
			items[0], items[1] = items[1], items[0]
		case "missing_activity":
			current["items"] = []any{user("task-input", taskPrompt), reply}
		case "child_path":
			child["source"] = nil
		case "activity_child":
			spawn["agentThreadId"] = "PRIVATE_OTHER"
		case "later_turn_supplied", "later_turn_fresh", "missing_prior_stream", "prior_failed", "prior_conflict", "prior_error_mismatch":
			prior := map[string]any{"id": "previous-child-turn", "status": "completed", "error": nil, "items": []any{agent("previous-reply", "initial complete")}}
			if strings.HasPrefix(mode, "prior_") {
				prior["status"], prior["error"] = "failed", map[string]any{"message": "PRIVATE_PRIOR_ERROR"}
			}
			child["turns"] = []any{clone(seed), prior, childTurn}
		case "child_later_failure":
			child["turns"] = append(childTurns, map[string]any{"id": "later", "status": "failed", "items": []any{}})
		case "child_later_turn":
			child["turns"] = append(childTurns, map[string]any{"id": "later", "status": "completed", "items": []any{agent("later", other)}})
		case "spawn_failed":
			spawn["kind"] = "completed"
		case "spawn_sender":
			spawn["agentPath"] = "/root/other"
		case "incidental_unrelated":
			current["items"] = append(current["items"].([]any), map[string]any{"type": "subAgentActivity", "id": "other", "kind": "started", "agentThreadId": "PRIVATE_OTHER", "agentPath": "/root/other"})
		case "unrelated_recipient":
			current["items"] = append(current["items"].([]any), map[string]any{"type": "subAgentActivity", "id": "other", "kind": "started", "agentThreadId": "PRIVATE_OTHER", "agentPath": "/root/other"})
			childTurn["items"] = []any{agent("child-reply", other)}
		case "many_valid", "multiple_results", "incidental_failed", "incidental_running", "cross_binding", "first_conflict_second_valid", "conflict_during_other_read":
			current["items"] = append(current["items"].([]any), map[string]any{"type": "subAgentActivity", "id": "second", "kind": "started", "agentThreadId": "PRIVATE_CHILD_TWO", "agentPath": "/root/second"})
		case "incidental_spawn_failure":
			current["items"] = append(current["items"].([]any), map[string]any{"type": "subAgentActivity", "id": "incidental", "kind": "interrupted", "agentThreadId": "PRIVATE_OTHER", "agentPath": "/root/other"})
		}
		if mode == "ambiguous_results" {
			childTurn["items"] = []any{agent("child-reply", nonce+" and "+other)}
		}
		if mode == "cross_binding" {
			childTurn["items"] = []any{agent("child-reply", other)}
		}
		if id == "PRIVATE_PARENT" {
			return map[string]any{"thread": parent}
		}
		if id == "PRIVATE_CHILD" {
			return map[string]any{"thread": child}
		}
		if id == "PRIVATE_CHILD_TWO" {
			copy := clone(child)
			copy["id"] = "PRIVATE_CHILD_TWO"
			copy["source"] = map[string]any{"subAgent": map[string]any{"thread_spawn": map[string]any{"agent_path": "/root/second"}}}
			second := map[string]any{"id": "second-child-turn", "status": "completed", "error": nil, "items": []any{agent("second-result", other)}}
			if mode == "first_conflict_second_valid" {
				second["items"] = []any{agent("second-result", nonce)}
			}
			if mode == "incidental_failed" {
				second["status"], second["items"] = "failed", []any{}
			}
			if mode == "incidental_running" {
				second["status"], second["items"] = "inProgress", []any{}
			}
			if mode == "cross_binding" {
				copy["modelProvider"] = "PRIVATE_OTHER"
				second["items"] = []any{agent("second-result", nonce)}
			}
			copy["turns"] = []any{clone(seed), second}
			return map[string]any{"thread": copy}
		}
		return map[string]any{"thread": map[string]any{"id": "PRIVATE_OTHER"}}
	}
	notify := func(method, id, turn string, item any) {
		params := map[string]any{"threadId": id}
		if method == "rawResponseItem/completed" {
			params["turnId"], params["item"] = turn, item
		} else {
			params["turn"] = item
		}
		_ = output.Encode(map[string]any{"method": method, "params": params})
	}
	emitChild := func(id, path, turn, text string) {
		if mode == "missing_stream" {
			return
		}
		if mode != "missing_child_start" && mode != "delayed_out_of_order" {
			notify("turn/started", id, turn, map[string]any{"id": turn, "status": "inProgress"})
		}
		inputItem := map[string]any{"type": "agent_message", "id": "raw-input", "author": "/root", "recipient": path, "content": []any{map[string]any{"type": "input_text", "text": "write a fresh UUID yourself"}}}
		if mode == "prompt_supplied" {
			inputItem["content"] = []any{map[string]any{"type": "input_text", "text": "Return " + nonce}}
		}
		if mode == "encrypted_input" {
			inputItem["content"] = []any{map[string]any{"type": "encrypted_content", "encrypted_content": "PRIVATE_SECRET"}}
		}
		if mode == "input_recipient" {
			inputItem["recipient"] = "/root/other"
		}
		if mode == "stream_byte_budget" {
			inputItem["content"] = []any{map[string]any{"type": "input_text", "text": strings.Repeat("x", 1<<20)}}
		}
		if mode != "missing_input" && mode != "input_after_result" && mode != "delayed_missing_input" {
			notify("rawResponseItem/completed", id, turn, inputItem)
		}
		if mode == "delayed_out_of_order" {
			notify("turn/started", id, turn, map[string]any{"id": turn, "status": "inProgress"})
		}
		if mode == "followup_supplied" || mode == "later_turn_supplied" {
			notify("rawResponseItem/completed", id, turn, map[string]any{"type": "agent_message", "id": "raw-followup", "author": "/root", "recipient": path, "content": []any{map[string]any{"type": "input_text", "text": "Use " + nonce}}})
		}
		rawID, rawTurn, rawText := id, turn, text
		if mode == "raw_child" {
			rawID = "PRIVATE_OTHER"
		}
		if mode == "raw_turn" {
			rawTurn = "PRIVATE_OTHER"
		}
		if mode == "raw_text" {
			rawText = other
		}
		if mode == "split_before_result_supplied" {
			notify("rawResponseItem/completed", id, turn, map[string]any{"type": "message", "id": "raw-commentary", "role": "assistant", "phase": "commentary", "content": []any{map[string]any{"type": "output_text", "text": "Preparing the result."}}})
			notify("rawResponseItem/completed", id, turn, map[string]any{"type": "agent_message", "author": "/root", "recipient": path, "content": []any{map[string]any{"type": "input_text", "text": "Use " + nonce}}})
		}
		if mode != "missing_raw_result" {
			notify("rawResponseItem/completed", rawID, rawTurn, map[string]any{"type": "message", "id": "raw-result-different-from-durable", "role": "assistant", "phase": "final_answer", "content": []any{map[string]any{"type": "output_text", "text": rawText}}})
		}
		if mode == "input_after_result" {
			notify("rawResponseItem/completed", id, turn, inputItem)
		}
		if mode == "after_result_mention" || mode == "split_after_result_mention" {
			notify("rawResponseItem/completed", id, turn, map[string]any{"type": "agent_message", "id": "raw-later", "author": "/root", "recipient": path, "content": []any{map[string]any{"type": "input_text", "text": "I received " + nonce}}})
		}
		if mode == "split_after_result_mention" || mode == "split_before_result_supplied" {
			notify("rawResponseItem/completed", id, turn, map[string]any{"type": "message", "id": "raw-tail", "role": "assistant", "phase": "final_answer", "content": []any{map[string]any{"type": "output_text", "text": "That is the result."}}})
		}
		if mode == "closed_child" {
			notify("thread/closed", id, "", nil)
		}
		if mode != "missing_child_terminal" && mode != "delayed_missing_terminal" {
			terminal := map[string]any{"id": turn, "status": "completed", "error": nil}
			if turn == "previous-child-turn" && strings.HasPrefix(mode, "prior_") {
				terminal["status"], terminal["error"] = "failed", map[string]any{"message": "PRIVATE_PRIOR_ERROR"}
				if mode == "prior_error_mismatch" {
					terminal["error"] = map[string]any{"message": "PRIVATE_OTHER_ERROR"}
				}
			}
			notify("turn/completed", id, turn, terminal)
			if turn == "previous-child-turn" && mode == "prior_conflict" {
				notify("turn/completed", id, turn, map[string]any{"id": turn, "status": "completed", "error": nil})
			}
		}
		if mode == "closed_after_result" {
			notify("thread/closed", id, "", nil)
		}
	}
	emitChildren := func() {
		if mode == "later_turn_supplied" || mode == "later_turn_fresh" || strings.HasPrefix(mode, "prior_") {
			emitChild("PRIVATE_CHILD", "/root/live_child", "previous-child-turn", "initial complete")
		}
		emitChild("PRIVATE_CHILD", "/root/live_child", "PRIVATE_CHILD_TURN", nonce)
		if mode == "many_valid" || mode == "multiple_results" || mode == "first_conflict_second_valid" {
			secondText := other
			if mode == "first_conflict_second_valid" {
				secondText = nonce
			}
			emitChild("PRIVATE_CHILD_TWO", "/root/second", "second-child-turn", secondText)
		}
	}

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
		activeID := "PRIVATE_PARENT_TURN"
		switch request.Method {
		case "initialize":
			result = map[string]any{"userAgent": "fixture"}
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
			if request.Params["model"] != "grok-4.7" || (mode != "startup" && request.Params["experimentalRawEvents"] != true) {
				return
			}
			result = map[string]any{"model": "grok-4.7", "modelProvider": "grok", "thread": map[string]any{"id": "PRIVATE_PARENT", "modelProvider": "grok"}}
			if mode == "wrong_start_provider" {
				result.(map[string]any)["modelProvider"] = "PRIVATE_OTHER"
			}
		case "turn/start":
			phase++
			if request.Params["threadId"] != "PRIVATE_PARENT" || phase > 2 {
				return
			}
			prompt := request.Params["input"].([]any)[0].(map[string]any)["text"].(string)
			if phase == 1 {
				if _, exists := request.Params["effort"]; exists {
					return
				}
				if mode != "startup" {
					activeID = "PRIVATE_SEED_TURN"
					text := "setup retained"
					if mode == "known_result" || mode == "known_legacy_result" {
						text = nonce
					}
					seed = map[string]any{"id": activeID, "status": "completed", "error": nil, "items": []any{user("seed-input", prompt), agent("seed-reply", text)}}
					if mode == "known_legacy_result" {
						seed["items"].([]any)[1].(map[string]any)["phase"] = ""
					}
					if mode == "known_commentary_result" {
						seed["items"] = append(seed["items"].([]any), map[string]any{"type": "agentMessage", "id": "seed-commentary", "phase": "commentary", "text": nonce})
					}
					if mode == "known_metadata" {
						seed["items"].([]any)[0].(map[string]any)["id"] = nonce
					}
					if mode == "known_user_result" {
						seed["items"].([]any)[0] = user("seed-input", "remember "+nonce)
					}
					_, _ = io.WriteString(trace, "setup/start\n")
				}
			} else {
				if request.Params["effort"] != "ultra" {
					return
				}
				_, _ = io.WriteString(trace, "task/start\n")
				taskPrompt = prompt
				if mode != "direct_stream" && mode != "stream_count_budget" && !strings.HasPrefix(mode, "delayed_") {
					emitChildren()
				}
			}
			result = map[string]any{"turn": map[string]any{"id": activeID, "status": "inProgress"}}
		case "thread/read":
			if mode == "late_conflict" && request.Params["threadId"] == "PRIVATE_CHILD" {
				_ = output.Encode(map[string]any{"method": "turn/completed", "params": map[string]any{"threadId": "PRIVATE_PARENT", "turn": map[string]any{"id": "PRIVATE_PARENT_TURN", "status": "failed"}}})
			}
			id := request.Params["threadId"].(string)
			if id == "PRIVATE_CHILD" && (mode == "child_terminal_failed" || mode == "child_terminal_interrupted" || mode == "child_terminal_error" || mode == "first_conflict_second_valid") || id == "PRIVATE_CHILD_TWO" && mode == "conflict_during_other_read" {
				status := "failed"
				var terminalError any
				if mode == "child_terminal_interrupted" {
					status = "interrupted"
				}
				if mode == "child_terminal_error" {
					status, terminalError = "completed", map[string]any{"message": "PRIVATE_ERROR"}
				}
				notify("turn/completed", "PRIVATE_CHILD", "PRIVATE_CHILD_TURN", map[string]any{"id": "PRIVATE_CHILD_TURN", "status": status, "error": terminalError})
			}
			if id == "PRIVATE_CHILD" && mode == "unrelated_terminal_failure" {
				notify("turn/completed", "PRIVATE_OTHER", "PRIVATE_CHILD_TURN", map[string]any{"id": "PRIVATE_CHILD_TURN", "status": "failed"})
			}
			result = snapshot(id)
		default:
			return
		}
		_ = output.Encode(map[string]any{"id": request.ID, "result": result})
		if request.Method == "thread/read" && request.Params["threadId"] == "PRIVATE_CHILD" && strings.HasPrefix(mode, "delayed_") {
			emitChildren()
		}
		if request.Method == "turn/start" && phase == 2 && (mode == "direct_stream" || mode == "stream_count_budget") {
			emitChildren()
			if mode == "stream_count_budget" {
				for i := 0; i < 513; i++ {
					notify("rawResponseItem/completed", "PRIVATE_OTHER", "other", map[string]any{"type": "reasoning"})
				}
			}
		}
		if request.Method == "turn/start" {
			terminal := map[string]any{"id": activeID, "status": "completed", "error": nil}
			threadID := "PRIVATE_PARENT"
			if phase == 2 {
				if mode == "failed" || mode == "interrupted" {
					terminal["status"] = mode
				}
				if mode == "wrong_thread" {
					threadID = "PRIVATE_OTHER"
				}
				if mode == "wrong_turn" {
					terminal["id"] = "PRIVATE_OTHER"
				}
				if mode == "missing_terminal" {
					return
				}
			}
			if phase == 1 && mode == "seed_failed" {
				terminal["status"] = "failed"
			}
			_ = output.Encode(map[string]any{"method": "turn/completed", "params": map[string]any{"threadId": threadID, "turn": terminal}})
			if phase == 2 && mode == "conflict" {
				terminal["status"] = "failed"
				_ = output.Encode(map[string]any{"method": "turn/completed", "params": map[string]any{"threadId": threadID, "turn": terminal}})
			}
			if phase == 2 && (mode == "wrong_thread" || mode == "wrong_turn") {
				return
			}
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
				t.Fatalf("catalog oracle mismatch: %+v %v", got, err)
			}
			if err == nil && (!got.ShippedCatalog || got.CatalogModels != 2 || got.Turns != 0 || got.Threads != 0) {
				t.Fatal("catalog-only scope changed")
			}
		})
	}
}

func shippedChildSuccess(mode string) bool {
	switch mode {
	case "known_metadata", "prior_failed", "inherited_diagnostic", "delayed_stream", "inherited_task_history", "closed_after_result", "split_after_result_mention", "direct_stream", "after_result_mention", "later_turn_fresh", "first_conflict_second_valid", "unrelated_terminal_failure", "ok", "many_valid", "multiple_results", "incidental_unrelated", "parent_prose", "incidental_failed", "incidental_running", "incidental_spawn_failure", "child_later_failure", "child_later_turn":
		return true
	}
	return false
}

func TestShippedStartupAndChildRejectIncompleteEvidence(t *testing.T) {
	// Final-fragment and closure cases keep freshness tied to the actual result,
	// while the child still needs a consistent completed terminal.
	modes := []string{"known_commentary_result", "known_user_result", "known_metadata", "known_legacy_result", "prior_failed", "prior_conflict", "prior_error_mismatch", "inherited_diagnostic", "delayed_stream", "delayed_out_of_order", "delayed_missing_input", "delayed_missing_terminal", "inherited_task_history", "inherited_supplied", "closed_after_result", "split_after_result_mention", "split_before_result_supplied", "direct_stream", "input_after_result", "missing_prior_stream", "stream_byte_budget", "stream_count_budget", "missing_activity", "child_path", "activity_child", "missing_stream", "missing_input", "missing_child_start", "missing_child_terminal", "missing_raw_result", "encrypted_input", "input_recipient", "raw_child", "raw_turn", "raw_text", "closed_child", "followup_supplied", "later_turn_supplied", "later_turn_fresh", "after_result_mention", "child_terminal_failed", "child_terminal_interrupted", "child_terminal_error", "conflict_during_other_read", "first_conflict_second_valid", "unrelated_terminal_failure", "ok", "many_valid", "multiple_results", "incidental_unrelated", "late_conflict", "parent_prose", "incidental_failed", "incidental_running", "incidental_spawn_failure", "child_later_failure", "child_later_turn", "child_no_history", "child_partial_history", "child_history_id", "child_history_item_id", "child_history_content", "child_history_order", "parent_history_changed", "cross_binding", "known_result", "prompt_supplied", "ambiguous_results", "seed_failed", "wrong_start_provider", "failed", "interrupted", "wrong_thread", "wrong_turn", "missing_terminal", "conflict", "stale", "duplicate_turn", "missing_reply", "wrong_history_provider", "wrong_history_model", "child_provider", "child_model", "child_parent", "child_fresh", "child_failed", "child_interrupted", "child_missing", "child_inherited", "child_mismatch", "spawn_failed", "spawn_sender", "unrelated_recipient"}
	for _, mode := range modes {
		t.Run(mode, func(t *testing.T) {
			t.Parallel()
			opts := fixtureOptions(t, "shipped:"+mode)
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			got, err := live.ShippedChildCollaboration(ctx, opts.Subject, opts.APIKey)
			if (err == nil) != shippedChildSuccess(mode) {
				t.Fatalf("child oracle mismatch: %+v %v", got, err)
			}
			if err == nil && (!got.ChildBound || !got.ChildCompleted || !got.ChildResultDelivered || got.Turns != 2 || got.SetupTurns != 1 || got.TaskTurns != 1 || !got.SetupCompleted) {
				t.Fatal("complete child proof absent")
			}
			if strings.Contains(fmt.Sprint(got, err), "PRIVATE_") {
				t.Fatal("private fixture data escaped")
			}
			var script struct{ Trace string }
			_ = json.Unmarshal([]byte(opts.APIKey), &script)
			trace, _ := os.ReadFile(script.Trace)
			if strings.Count(string(trace), "turn/start\n") > 2 || strings.Count(string(trace), "setup/start\n") > 1 || strings.Count(string(trace), "task/start\n") > 1 {
				t.Fatal("semantic turn resubmitted")
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

func TestNativeShippedCatalog(t *testing.T) {
	binary := os.Getenv("GROK_LIVE_NATIVE_BIN")
	if binary == "" {
		t.Skip("activated by required runtime job")
	}
	subject := binarySubject(t, binary)
	subject.SourceSHA, subject.HarnessSHA = os.Getenv("GITHUB_SHA"), os.Getenv("GITHUB_SHA")
	ctx, cancel := context.WithTimeout(context.Background(), time.Minute)
	defer cancel()
	got, err := live.ShippedCatalog(ctx, subject, "catalog-only-fixture")
	if err != nil || !got.ShippedCatalog || got.CatalogModels != 2 || got.Turns != 0 {
		t.Fatalf("native shipped catalog: %+v %v", got, err)
	}
}
