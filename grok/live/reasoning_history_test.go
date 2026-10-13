package live_test

import (
	"bufio"
	"context"
	"encoding/json"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

const historyCanary = "PRIVATE_HISTORY_CANARY"

var historyNonce = regexp.MustCompile(`^[0-9a-f]{64}$`)
var historyTokenShape = regexp.MustCompile(`[0-9a-f]{64}`)

// This child speaks only public stdio. Its trace contains static assertion markers,
// never the fresh token, prompts, identifiers, reasoning, ciphertext or raw frames.
func fakeHistoryServer() {
	var script struct{ Mode, Trace string }
	if json.Unmarshal([]byte(os.Getenv("GROK_API_KEY")), &script) != nil {
		return
	}
	trace, err := os.OpenFile(script.Trace, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
	if err != nil {
		return
	}
	defer trace.Close()
	// SDK cleanup interrupts the process before closing stdin. Keep that signal
	// from discarding a queued typed denial or the ensuing safe EOF trace.
	signal.Notify(make(chan os.Signal, 1), os.Interrupt)
	mark := func(s string) { _, _ = io.WriteString(trace, s+"\n") }
	mark("process")
	_, _ = io.WriteString(os.Stderr, historyCanary)
	input, output := bufio.NewScanner(os.Stdin), json.NewEncoder(os.Stdout)
	type packet struct {
		raw    string
		ID     json.RawMessage `json:"id"`
		Method string          `json:"method"`
		Params map[string]any  `json:"params"`
		Result struct {
			Success      *bool                         `json:"success"`
			ContentItems []struct{ Type, Text string } `json:"contentItems"`
			Decision     string                        `json:"decision"`
		} `json:"result"`
		Error struct{ Code int } `json:"error"`
	}
	read := func() (packet, bool) {
		var p packet
		ok := input.Scan() && json.Unmarshal(input.Bytes(), &p) == nil
		p.raw = string(input.Bytes())
		return p, ok
	}
	send := func(v any) { _ = output.Encode(v) }
	reply := func(p packet, result any) { send(map[string]any{"id": p.ID, "result": result}) }
	mode := strings.TrimPrefix(script.Mode, "history:")
	phase, token, firstPrompt := 0, "", ""
	var boundThread map[string]any
	for {
		p, ok := read()
		if !ok {
			return
		}
		switch p.Method {
		case "initialize":
			mark("initialize")
			capabilities, _ := p.Params["capabilities"].(map[string]any)
			if capabilities["experimentalApi"] != true {
				return
			}
			mark("initialize_valid")
			reply(p, map[string]any{"userAgent": "fixture", "codexHome": os.Getenv("CODEX_HOME"), "platformFamily": "unix", "platformOs": "linux"})
		case "initialized":
			mark("initialized")
		case "model/list":
			if mode != "shipped" || !copiedShippedAssets() {
				return
			}
			reply(p, fixtureShippedCatalog())
		case "thread/start":
			mark("thread/start")
			tools, _ := p.Params["dynamicTools"].([]any)
			if len(tools) != 1 || p.Params["experimentalRawEvents"] != true || p.Params["modelProvider"] != "grok" || p.Params["environments"] != nil {
				return
			}
			tool, _ := tools[0].(map[string]any)
			schema, _ := tool["inputSchema"].(map[string]any)
			if tool["type"] != "function" || tool["name"] != "grok_history_probe" || schema["type"] != "object" {
				return
			}
			cwd, _ := os.Getwd()
			catalog, readErr := os.ReadFile(filepath.Join(os.Getenv("CODEX_HOME"), "models.json"))
			if readErr != nil || p.Params["cwd"] != cwd || (mode != "shipped" && (!strings.Contains(string(catalog), `"supported_reasoning_levels":[]`) || !strings.Contains(string(catalog), `"supports_reasoning_summary_parameter":false`))) || (mode == "shipped" && (!copiedShippedAssets() || p.Params["model"] != "grok-4.6")) {
				return
			}
			mark("thread_valid")
			model, provider, threadProvider := p.Params["model"], "grok", "grok"
			if mode == "wrong_model" {
				model = historyCanary
			}
			if mode == "wrong_provider" {
				provider = historyCanary
			}
			if mode == "wrong_thread_provider" {
				threadProvider = historyCanary
			}
			boundThread = map[string]any{
				"model": model, "modelProvider": provider, "approvalPolicy": "never", "approvalsReviewer": "user", "cwd": cwd, "sandbox": map[string]any{"type": "readOnly"},
				"thread": map[string]any{"id": "PRIVATE_THREAD", "sessionId": "PRIVATE_SESSION", "modelProvider": threadProvider, "cliVersion": "fixture", "createdAt": 1, "updatedAt": 1, "cwd": cwd, "ephemeral": false, "preview": "", "projectId": nil, "source": "appServer", "status": map[string]any{"type": "idle"}, "turns": []any{}},
			}
			reply(p, boundThread)
		case "thread/resume":
			mark("thread/resume")
			if mode == "shipped" || phase != 1 || boundThread == nil || len(p.Params) != 5 || p.Params["threadId"] != "PRIVATE_THREAD" || p.Params["model"] != boundThread["model"] || p.Params["modelProvider"] != "grok" || p.Params["cwd"] != boundThread["cwd"] || p.Params["excludeTurns"] != true || token != "" && strings.Contains(p.raw, token) {
				return
			}
			mark("resume_valid")
			reply(p, boundThread)
		case "turn/start":
			mark("turn/start")
			phase++
			_, effortPresent := p.Params["effort"]
			effortValid := p.Params["effort"] == "medium" && len(p.Params) == 3
			if mode == "no_effort" {
				effortValid = !effortPresent && len(p.Params) == 2
			}
			if phase > 2 || p.Params["threadId"] != "PRIVATE_THREAD" || !effortValid || p.Params["summary"] != nil || p.Params["environments"] != nil {
				return
			}
			items, _ := p.Params["input"].([]any)
			if len(items) != 1 {
				return
			}
			prompt, _ := items[0].(map[string]any)
			text, _ := prompt["text"].(string)
			if prompt["type"] != "text" || strings.TrimSpace(text) == "" || strings.Contains(text, historyCanary) || token != "" && strings.Contains(p.raw, token) {
				return
			}
			if phase == 1 {
				firstPrompt = text
				if !strings.Contains(text, "grok_history_probe") || !strings.Contains(strings.ToLower(text), "reason") {
					return
				}
			} else if text == firstPrompt || !strings.Contains(strings.ToLower(text), "history") {
				return
			}
			mark("turn_valid")
			scenario := ""
			if phase == 1 && !strings.HasPrefix(mode, "next_") || phase == 2 && strings.HasPrefix(mode, "next_") {
				scenario = strings.TrimPrefix(mode, "next_")
			}
			if mode == "identity_limit" || mode == "identity_limit_summary" {
				// Fill independent turn-specific identity sets to prove the budget resets.
				scenario = mode
			}
			turn := "PRIVATE_TURN_ONE"
			if phase == 2 && scenario != "reused_turn" {
				turn = "PRIVATE_TURN_TWO"
			}
			started := map[string]any{"turn": map[string]any{"id": turn, "status": "inProgress", "items": []any{}}}
			early := scenario == "early" || scenario == "inline" || scenario == "inline_conflict" || scenario == "early_unsupported" || scenario == "provisional_mismatch"
			if !early {
				reply(p, started)
			}
			if scenario == "hang" {
				time.Sleep(time.Hour)
				return
			}
			if scenario == "eof" {
				return
			}
			if scenario == "malformed" {
				_, _ = io.WriteString(os.Stdout, historyCanary+"\n")
				return
			}
			toolItem := map[string]any{"type": "dynamicToolCall", "id": "PRIVATE_CALL", "tool": "grok_history_probe", "namespace": nil, "arguments": map[string]any{}, "status": "completed", "success": true}
			calls := 0
			if phase == 1 && scenario != "no_request" || phase == 2 && scenario == "denial" {
				calls = 1
			}
			if scenario == "repeated" {
				calls = 2
			}
			if scenario == "tool_identity_limit" || scenario == "tool_identity_overflow" {
				calls = 4097
			}
			for call := 0; call < calls; call++ {
				params := map[string]any{"threadId": "PRIVATE_THREAD", "turnId": turn, "callId": "PRIVATE_CALL", "tool": "grok_history_probe", "namespace": nil, "arguments": map[string]any{}}
				if call == 1 {
					params["callId"] = "PRIVATE_REPEAT"
				}
				if scenario == "tool_identity_limit" || scenario == "tool_identity_overflow" {
					identity := call
					if scenario == "tool_identity_limit" && call == 4096 {
						// A duplicate at capacity must preserve the accepted result
						// without spending another identity slot.
						identity = 0
					}
					params["callId"] = "PRIVATE_CALL_" + strconv.Itoa(identity)
				}
				method := "item/tool/call"
				switch scenario {
				case "request_thread":
					params["threadId"] = historyCanary
				case "request_turn":
					params["turnId"] = historyCanary
				case "request_namespace":
					params["namespace"] = historyCanary
				case "request_empty_namespace":
					params["namespace"] = ""
				case "request_tool":
					params["tool"] = historyCanary
				case "empty_call":
					params["callId"] = ""
				case "unsupported", "early_unsupported":
					method = "item/commandExecution/requestApproval"
					params = map[string]any{"threadId": "PRIVATE_THREAD", "turnId": turn, "itemId": "PRIVATE_APPROVAL", "startedAtMs": 1, "command": historyCanary}
				}
				if scenario == "tool_identity_overflow" && call == 4096 {
					mark("tool_identity_overflow_sent")
				}
				send(map[string]any{"id": 99 + call, "method": method, "params": params})
				result, valid := read()
				if !valid || string(result.ID) != strconv.Itoa(99+call) || result.Method != "" {
					return
				}
				if scenario == "tool_identity_overflow" && call == 4096 {
					if result.Result.Success == nil || *result.Result.Success || result.Error.Code != 0 || strings.Contains(result.raw, token) || historyTokenShape.MatchString(result.raw) {
						return
					}
					mark("tool_identity_refused")
					if _, ok := read(); ok {
						mark("unexpected_method")
					}
					mark("tool_identity_input_closed")
					return
				}
				if strings.HasPrefix(scenario, "request_") || scenario == "empty_call" || scenario == "unsupported" || scenario == "early_unsupported" {
					// A typed denial or an RPC refusal must never return success or
					// create a secret. Exact transport error codes are SDK-owned.
					if (result.Error.Code != 0 || result.Result.Success != nil && !*result.Result.Success || result.Result.Decision == "decline" || result.Result.Decision == "cancel") && (result.Result.Success == nil || !*result.Result.Success) && !historyTokenShape.MatchString(result.raw) && token == "" {
						mark("request_refused")
					}
					if scenario == "request_turn" && result.Result.Success != nil && *result.Result.Success && result.Error.Code == 0 && len(result.Result.ContentItems) == 1 && result.Result.ContentItems[0].Type == "inputText" && historyNonce.MatchString(result.Result.ContentItems[0].Text) && !strings.Contains(firstPrompt, result.Result.ContentItems[0].Text) {
						// A concurrent SDK callback can run before turn/start's
						// response becomes application-visible. The provisional
						// identity must then fail reconciliation with the actual turn.
						mark("provisional_token_valid")
					}
					if early {
						reply(p, started)
					}
					// Keep the connection alive so EOF cannot disguise approval or
					// attribution mistakes as an unsuccessful proof.
					if _, ok := read(); ok {
						mark("unexpected_method")
					}
					mark("request_input_closed")
					return
				}
				if phase == 2 {
					if result.Result.Success == nil || *result.Result.Success || len(result.Result.ContentItems) != 1 || result.Result.ContentItems[0].Type != "inputText" || result.Result.ContentItems[0].Text == "" || strings.Contains(result.raw, token) || result.Error.Code != 0 {
						return
					}
					mark("denial_valid")
					continue
				}
				if result.Result.Success == nil || !*result.Result.Success || result.Error.Code != 0 || len(result.Result.ContentItems) != 1 || result.Result.ContentItems[0].Type != "inputText" {
					return
				}
				value := result.Result.ContentItems[0].Text
				if !historyNonce.MatchString(value) || strings.Contains(firstPrompt, value) || token != "" && token != value {
					return
				}
				token = value
				toolItem["id"] = params["callId"]
				toolItem["contentItems"] = []any{map[string]any{"type": "inputText", "text": token}}
				mark("token_valid")
			}
			if scenario == "provisional_mismatch" {
				started["turn"].(map[string]any)["id"] = historyCanary
			}
			threadID, eventTurn := "PRIVATE_THREAD", turn
			if scenario == "wrong_thread" {
				threadID = historyCanary
			}
			if scenario == "wrong_turn" {
				eventTurn = historyCanary
			}
			emit := func(method string, item any) {
				itemThread, itemTurn := threadID, eventTurn
				value, _ := item.(map[string]any)
				prefix := ""
				if method == "rawResponseItem/completed" {
					prefix = "cipher_"
				} else if value["type"] == "dynamicToolCall" {
					prefix = "tool_"
				} else if value["type"] == "agentMessage" {
					prefix = "reply_"
				}
				if scenario == prefix+"wrong_thread" {
					itemThread = historyCanary
				}
				if scenario == prefix+"wrong_turn" {
					itemTurn = historyCanary
				}
				params := map[string]any{"threadId": itemThread, "turnId": itemTurn, "item": item}
				if method == "item/completed" {
					params["completedAtMs"] = 1
				}
				send(map[string]any{"method": method, "params": params})
			}
			reason := map[string]any{"type": "reasoning", "id": "PRIVATE_REASONING", "summary": []any{}, "content": []any{historyCanary}}
			cipher := map[string]any{"type": "reasoning", "id": "PRIVATE_CIPHER", "summary": []any{}, "encrypted_content": historyCanary}
			if scenario == "no_reason" {
				cipher = map[string]any{"type": "message", "id": "PRIVATE_CIPHER", "role": "assistant", "content": []any{}}
				send(map[string]any{"method": "item/reasoning/textDelta", "params": map[string]any{"threadId": threadID, "turnId": eventTurn, "itemId": "PRIVATE_REASONING", "delta": historyCanary, "contentIndex": 0}})
			}
			if scenario == "no_cipher" {
				cipher["encrypted_content"] = ""
			}
			if scenario == "cipher_missing_identity" {
				delete(cipher, "id")
			}
			if scenario == "cipher_empty_identity" {
				cipher["id"] = ""
			}
			if phase == 1 && scenario != "missing_cipher" {
				emit("rawResponseItem/completed", cipher)
			}
			if early && scenario != "inline" && scenario != "inline_conflict" {
				reply(p, started)
			}
			if scenario == "tool_failed" {
				toolItem["success"] = false
			}
			if scenario == "tool_partial" {
				toolItem["status"] = "inProgress"
			}
			if scenario == "tool_namespace" {
				toolItem["namespace"] = historyCanary
			}
			if scenario == "tool_empty_namespace" {
				toolItem["namespace"] = ""
			}
			if scenario == "wrong_tool" {
				toolItem["tool"] = historyCanary
			}
			if scenario == "tool_wrong_call" {
				toolItem["id"] = "PRIVATE_UNREQUESTED_CALL"
			}
			if scenario == "wrong_output" {
				toolItem["contentItems"] = []any{map[string]any{"type": "inputText", "text": historyCanary}}
			}
			agent := map[string]any{"type": "agentMessage", "id": "PRIVATE_REPLY", "phase": "final_answer", "text": token}
			if scenario == "legacy" || scenario == "legacy_superseded" || scenario == "legacy_replaced" || scenario == "legacy_commentary" {
				delete(agent, "phase")
			}
			if scenario == "no_reply" || scenario == "delta" {
				agent["text"] = ""
			}
			if scenario == "wrong_reply" || scenario == "legacy_replaced" {
				agent["text"] = historyCanary
			}
			if scenario == "delta" {
				send(map[string]any{"method": "item/agentMessage/delta", "params": map[string]any{"threadId": threadID, "turnId": eventTurn, "itemId": "PRIVATE_REPLY", "delta": token}})
			}
			if scenario == "commentary" {
				agent["phase"] = "commentary"
			}
			terminalItems := []any{}
			if phase == 1 && scenario != "no_reason" && scenario != "encrypted_only" {
				terminalItems = append(terminalItems, reason)
			}
			if phase == 1 && scenario != "no_tool" {
				terminalItems = append(terminalItems, toolItem)
			}
			if scenario != "missing_reply" {
				terminalItems = append(terminalItems, agent)
			}
			if scenario == "item" || scenario == "no_terminal" || scenario == "early" || scenario == "late" || strings.HasPrefix(scenario, "legacy_") || scenario == "summary_replay" || scenario == "delta" || strings.HasPrefix(scenario, "tool_wrong_") || strings.HasPrefix(scenario, "reply_wrong_") {
				for _, item := range terminalItems {
					if scenario != "late" {
						emit("item/completed", item)
					}
				}
				terminalItems = []any{}
			}
			if strings.HasPrefix(scenario, "legacy_") || scenario == "summary_replay" {
				last := map[string]any{"type": "agentMessage", "id": "PRIVATE_LAST_REPLY", "text": historyCanary}
				if scenario == "legacy_replaced" {
					last["text"] = token
				}
				if scenario == "legacy_commentary" {
					last["phase"] = "commentary"
				}
				if scenario == "summary_replay" {
					terminalItems = []any{last}
				} else {
					emit("item/completed", last)
				}
			}
			var afterTerminal any
			if strings.HasPrefix(scenario, "identity_") {
				mark("identity_sequence")
				for _, item := range terminalItems {
					if item.(map[string]any)["type"] != "agentMessage" {
						emit("item/completed", item)
					}
				}
				terminalItems = []any{}
				assistant := func(id, phase, text string) map[string]any {
					item := map[string]any{"type": "agentMessage", "id": id, "text": text}
					if phase != "" {
						item["phase"] = phase
					}
					return item
				}
				a, b := "PRIVATE_REPLY_A", "PRIVATE_REPLY_B"
				switch scenario {
				case "identity_replay", "identity_replay_final":
					phase := ""
					if scenario == "identity_replay_final" {
						phase = "final_answer"
					}
					emit("item/completed", assistant(a, phase, token))
					emit("item/completed", assistant(b, phase, historyCanary))
					// Leave the child connected after this post-terminal replay so
					// cleanup errors cannot mask incorrectly accepted stale recall.
					afterTerminal = assistant(a, phase, token)
				case "identity_delayed_final":
					emit("item/completed", assistant(a, "commentary", token))
					afterTerminal = assistant(b, "final_answer", token)
				case "identity_duplicate":
					emit("item/completed", assistant(a, "final_answer", token))
					// The fingerprint is unchanged despite a different text size.
					emit("item/completed", assistant(a, "final_answer", token+historyCanary))
				case "identity_conflict_recall":
					emit("item/completed", assistant(a, "final_answer", historyCanary))
					emit("item/completed", assistant(a, "final_answer", token))
				case "identity_conflict_new_final", "identity_conflict_eligibility":
					phase, text := "final_answer", token
					if scenario == "identity_conflict_eligibility" {
						// Both occurrences do not recall; eligibility alone conflicts.
						phase, text = "commentary", historyCanary
					}
					emit("item/completed", assistant(a, phase, text))
					emit("item/completed", assistant(a, "final_answer", historyCanary))
					// A new recalling candidate cannot clear an earlier identity conflict.
					if scenario == "identity_conflict_new_final" {
						afterTerminal = assistant(b, "final_answer", token)
					} else {
						emit("item/completed", assistant(b, "final_answer", token))
					}
				case "identity_authority_duplicate_recall", "identity_authority_changed_recall", "identity_authority_duplicate_no_recall", "identity_authority_changed_no_recall", "identity_authority_last_recall", "identity_authority_last_no_recall":
					initial, changed, final := token, historyCanary, token
					if scenario == "identity_authority_changed_recall" || scenario == "identity_authority_duplicate_no_recall" {
						initial, changed = historyCanary, token
					}
					if strings.HasSuffix(scenario, "no_recall") {
						final = historyCanary
					}
					emit("item/completed", assistant(a, "final_answer", initial))
					emit("item/completed", assistant(a, "final_answer", changed))
					terminalItems = []any{assistant(a, "final_answer", final)}
					if strings.Contains(scenario, "_last_") {
						other := historyCanary
						if final == historyCanary {
							other = token
						}
						terminalItems = []any{assistant(a, "final_answer", other), assistant(b, "final_answer", final)}
					}
				case "identity_missing_item", "identity_empty_item", "identity_missing_summary", "identity_empty_summary":
					missing := assistant("", "final_answer", token)
					if strings.Contains(scenario, "_missing_") {
						delete(missing, "id")
					}
					if strings.HasSuffix(scenario, "_summary") {
						terminalItems = []any{missing}
					} else {
						emit("item/completed", missing)
						if _, ok := read(); ok {
							mark("unexpected_method")
						}
						return
					}
				case "identity_limit", "identity_limit_summary", "identity_overflow", "identity_limit_summary_no_recall":
					count := 4096
					if scenario != "identity_limit" {
						// A terminal summary does not retain another notification identity.
						count++
					}
					for index := 0; index < count; index++ {
						text := historyCanary
						if index == count-1 && scenario != "identity_limit_summary_no_recall" {
							text = token
						}
						item := assistant("PRIVATE_REPLY_"+turn+"_"+strconv.Itoa(index), "final_answer", text)
						if strings.Contains(scenario, "_summary") && index == count-1 {
							terminalItems = []any{item}
						} else {
							emit("item/completed", item)
						}
					}
					if scenario == "identity_overflow" {
						// Do not close stdout: rejection must precede EOF or cancellation.
						mark("identity_overflow_sent")
						if _, ok := read(); ok {
							mark("unexpected_method")
						}
						mark("identity_input_closed")
						return
					}
					if scenario == "identity_limit" {
						// At capacity, an old duplicate must neither spend budget nor supersede.
						emit("item/completed", assistant("PRIVATE_REPLY_"+turn+"_0", "final_answer", historyCanary))
					}
				}
			}
			completed := map[string]any{"id": eventTurn, "status": "completed", "error": nil, "items": terminalItems}
			if scenario == "failed" || scenario == "partial" {
				completed["status"] = "failed"
			}
			if scenario == "partial" {
				completed["status"] = "inProgress"
			}
			if scenario == "error" {
				completed["error"] = map[string]any{"message": historyCanary}
			}
			if scenario == "inline" || scenario == "inline_conflict" {
				if scenario == "inline_conflict" {
					send(map[string]any{"method": "turn/completed", "params": map[string]any{"threadId": threadID, "turn": map[string]any{"id": turn, "status": "failed", "error": map[string]any{"message": historyCanary}, "items": []any{}}}})
				}
				reply(p, map[string]any{"turn": completed})
			} else if scenario != "no_terminal" {
				send(map[string]any{"method": "turn/completed", "params": map[string]any{"threadId": threadID, "turn": completed}})
			}
			if afterTerminal != nil {
				emit("item/completed", afterTerminal)
				mark("after_terminal_sent")
			}
			if scenario == "late" {
				emit("item/completed", reason)
				emit("item/completed", toolItem)
				emit("item/completed", agent)
			}
			if scenario == "summary_replay" {
				emit("item/completed", agent)
			}
			// All ordinary fixtures remain available until the harness closes its
			// input. Only explicit eof/malformed controls end transport early.
		default:
			mark("unexpected_method")
			return
		}
	}
}

func historySuccess(mode string) bool {
	switch strings.TrimPrefix(mode, "next_") {
	case "identity_delayed_final", "identity_duplicate", "identity_authority_duplicate_recall", "identity_authority_changed_recall", "identity_authority_last_recall", "identity_limit", "identity_limit_summary":
		return true
	}
	switch mode {
	case "shipped", "no_effort", "turn", "item", "early", "late", "inline", "encrypted_only", "repeated", "tool_identity_limit", "legacy", "legacy_replaced", "next_denial", "next_item", "next_inline", "next_late", "next_legacy":
		return true
	}
	return false
}

func TestReasoningHistoryPublicProtocol(t *testing.T) {
	var capacity sync.Mutex
	modes := []string{"no_effort", "turn", "item", "early", "late", "inline", "encrypted_only", "repeated", "legacy", "legacy_replaced", "next_denial", "next_item", "next_inline", "next_late", "next_legacy",
		"wrong_model", "wrong_provider", "wrong_thread_provider", "request_thread", "request_turn", "request_namespace", "request_empty_namespace", "request_tool", "empty_call", "unsupported", "early_unsupported", "inline_conflict", "provisional_mismatch", "wrong_thread", "wrong_turn",
		"no_reason", "no_cipher", "missing_cipher", "cipher_missing_identity", "cipher_empty_identity", "cipher_wrong_thread", "cipher_wrong_turn", "no_request", "no_tool", "tool_failed", "tool_partial", "tool_namespace", "tool_empty_namespace", "tool_wrong_thread", "tool_wrong_turn", "tool_wrong_call", "tool_identity_limit", "tool_identity_overflow", "reply_wrong_thread", "reply_wrong_turn", "wrong_tool", "wrong_output", "no_reply", "missing_reply", "wrong_reply", "commentary", "delta", "legacy_superseded", "legacy_commentary", "summary_replay", "no_terminal", "failed", "partial", "error", "eof", "malformed", "hang",
		"next_wrong_thread", "next_wrong_turn", "next_reused_turn", "next_no_reply", "next_missing_reply", "next_wrong_reply", "next_commentary", "next_legacy_superseded", "next_summary_replay", "next_no_terminal", "next_failed", "next_error", "next_eof", "next_inline_conflict"}
	for _, mode := range []string{
		"identity_replay", "identity_replay_final", "identity_delayed_final", "identity_duplicate",
		"identity_conflict_recall", "identity_conflict_new_final", "identity_conflict_eligibility",
		"identity_authority_duplicate_recall", "identity_authority_changed_recall", "identity_authority_duplicate_no_recall", "identity_authority_changed_no_recall", "identity_authority_last_recall", "identity_authority_last_no_recall",
		"identity_missing_item", "identity_empty_item", "identity_missing_summary", "identity_empty_summary",
		"identity_limit", "identity_limit_summary", "identity_overflow", "identity_limit_summary_no_recall",
	} {
		modes = append(modes, mode, "next_"+mode)
	}
	for _, model := range []string{providerfixture.PrimaryModel, providerfixture.PinnedModel} {
		for _, mode := range modes {
			t.Run(model+"/"+mode, func(t *testing.T) {
				t.Parallel()
				if strings.Contains(mode, "identity_limit") || strings.Contains(mode, "identity_overflow") {
					// Pinned-SDK decoding and retained snapshots have real costs.
					// Serialize floods before creating their original bounded context.
					capacity.Lock()
					defer capacity.Unlock()
				}
				options := fixtureOptions(t, "history:"+mode)
				options.Model = model
				deadline := 5 * time.Second
				if !historySuccess(mode) {
					deadline = 2 * time.Second
				}
				if strings.Contains(mode, "identity_limit") || strings.Contains(mode, "identity_overflow") {
					deadline = 15 * time.Second
				}
				ctx, cancel := context.WithTimeout(context.Background(), deadline)
				defer cancel()
				began := time.Now()
				runner := live.ReasoningHistory
				if mode == "no_effort" {
					runner = live.ReasoningHistoryWithoutEffort
				}
				got, err := runner(ctx, options)
				if (err == nil) != historySuccess(mode) {
					t.Fatalf("history proof result disagrees with public protocol fixture: evidence=%+v failure=%+v", got, live.DescribeFailure(err))
				}
				if time.Since(began) > deadline+4*time.Second {
					t.Fatal("child cleanup exceeded bounded allowance")
				}
				if _, parseErr := time.Parse(time.RFC3339, got.ObservedAt); parseErr != nil {
					t.Fatal("missing observation timestamp")
				}
				encoded, _ := json.Marshal(got)
				safe := string(encoded)
				if err != nil {
					safe += err.Error()
					if historyTokenShape.MatchString(err.Error()) {
						t.Fatal("token-shaped error escaped")
					}
				}
				for _, secret := range []string{historyCanary, "PRIVATE_THREAD", "PRIVATE_SESSION", "PRIVATE_TURN", "PRIVATE_CALL", "PRIVATE_REPEAT", "PRIVATE_UNREQUESTED_CALL", "PRIVATE_APPROVAL", "PRIVATE_REPLY", "PRIVATE_LAST_REPLY", "PRIVATE_REASONING", "PRIVATE_CIPHER", options.APIKey, options.Subject.Binary, options.BaseURL} {
					if strings.Contains(safe, secret) {
						t.Fatal("private history evidence escaped")
					}
				}
				if got.SHA256 != options.Subject.SHA256 || got.SourceSHA != options.Subject.SourceSHA || got.HarnessSHA != options.Subject.HarnessSHA || got.Target != "test-host" || got.Environment != "deterministic" || got.Model != model || !regexp.MustCompile(`^(initialized|thread_bound|turn_submitted|first_turn_completed|first_turn_proven|continuation_submitted|continuation_completed|history_recalled)$`).MatchString(got.Stage) {
					t.Fatal("unsafe or missing subject evidence")
				}
				wantTurns := 1
				if mode == "wrong_model" || mode == "wrong_provider" || mode == "wrong_thread_provider" {
					wantTurns = 0
				} else if historySuccess(mode) || strings.HasPrefix(mode, "next_") {
					wantTurns = 2
				}
				var script struct{ Trace string }
				_ = json.Unmarshal([]byte(options.APIKey), &script)
				trace, readErr := os.ReadFile(script.Trace)
				if readErr != nil {
					t.Fatal("missing safe fixture trace")
				}
				counts := map[string]int{}
				for _, marker := range strings.Fields(string(trace)) {
					counts[marker]++
				}
				for _, marker := range []string{"process", "initialize", "initialize_valid", "initialized", "thread/start", "thread_valid"} {
					if counts[marker] != 1 {
						t.Fatalf("invalid %s invocation budget", marker)
					}
				}
				if counts["turn/start"] != wantTurns || counts["turn_valid"] != wantTurns || counts["unexpected_method"] != 0 || got.Processes != 1 || got.Initializations != 1 || got.Threads != 1 || got.Turns != wantTurns {
					t.Fatalf("semantic submission or invocation budget violated: markers=%v evidence=%+v failure=%+v", counts, got, live.DescribeFailure(err))
				}
				wantResumes := 0
				if wantTurns == 2 {
					wantResumes = 1
				}
				if counts["thread/resume"] != wantResumes || counts["resume_valid"] != wantResumes {
					t.Fatal("SDK continuation did not preserve the original thread and fixture")
				}
				if strings.HasPrefix(mode, "request_") && mode != "request_turn" || mode == "empty_call" || mode == "unsupported" || mode == "early_unsupported" {
					if counts["request_refused"] != 1 {
						t.Fatal("unrelated request was not explicitly refused")
					}
				}
				if mode == "request_turn" {
					wantCode := "history_tool_request_unattributed"
					if counts["provisional_token_valid"] == 1 {
						wantCode = "turn_identity_missing"
					}
					if counts["request_refused"]+counts["provisional_token_valid"] != 1 || counts["request_input_closed"] != 1 || got.ToolCalls != counts["provisional_token_valid"] || got.CompletedTools != 0 || got.FirstCompleted || got.FirstRecall || got.Completed || got.Recalled || live.DescribeFailure(err).Code != wantCode || ctx.Err() != nil {
						t.Fatal("mistaken provisional tool turn was not reconciled before proof or deadline")
					}
				}
				if strings.HasPrefix(mode, "next_") && (!got.FirstCompleted || !got.FirstRecall || got.FirstReplyBytes != 64 || got.ReasoningItems < 1 || got.EncryptedItems < 1 || got.CompletedTools < 1) {
					t.Fatal("continuation failure corrupted the completed first-turn proof")
				}
				if mode == "no_terminal" || mode == "next_no_terminal" {
					completed, recall, replyBytes := got.FirstCompleted, got.FirstRecall, got.FirstReplyBytes
					if mode == "next_no_terminal" {
						completed, recall, replyBytes = got.Completed, got.Recalled, got.ReplyBytes
					}
					// The fixture emits one raw and one typed reasoning occurrence
					// and one completed tool. Failure recovery must not add replays.
					if live.DescribeFailure(err).Code != "deadline_exceeded" || got.ReasoningItems != 2 || got.EncryptedItems != 1 || got.CompletedTools != 1 || got.ToolCalls != 1 || completed || !recall || replyBytes != 64 {
						t.Fatal("failure recovery duplicated or lost real partial evidence")
					}
				}
				if mode == "cipher_missing_identity" || mode == "cipher_empty_identity" {
					// Raw reasoning permits an absent ID on the wire, but cannot
					// establish encrypted-history authority without that identity.
					if live.DescribeFailure(err).Code != "deadline_exceeded" || ctx.Err() == nil || got.EncryptedItems != 0 || got.ReasoningItems != 1 || got.ToolCalls != 1 || got.CompletedTools != 1 || counts["token_valid"] != 1 || !got.FirstCompleted || !got.FirstRecall || got.FirstReplyBytes != 64 || got.Completed || got.Recalled || got.ReplyBytes != 0 || got.Stage == "first_turn_proven" {
						t.Fatalf("raw reasoning without identity falsely established encrypted history: evidence=%+v failure=%+v", got, live.DescribeFailure(err))
					}
				}
				if mode == "tool_wrong_call" && (got.CompletedTools != 0 || got.ToolCalls != 1 || counts["token_valid"] != 1) {
					t.Fatal("completed tool output without an accepted request identity was credited")
				}
				if mode == "tool_identity_overflow" && (live.DescribeFailure(err).Code != "history_tool_identity_budget_exceeded" || ctx.Err() != nil || got.ToolCalls != 4096 || counts["token_valid"] != 4096 || counts["tool_identity_overflow_sent"] != 1 || counts["tool_identity_refused"] != 1 || counts["tool_identity_input_closed"] != 1 || got.FirstCompleted || got.FirstRecall || got.CompletedTools != 0) {
					t.Fatalf("tool identity budget did not deny before EOF or deadline: markers=%v evidence=%+v failure=%+v", counts, got, live.DescribeFailure(err))
				}
				if scenario := strings.TrimPrefix(mode, "next_"); strings.HasPrefix(scenario, "identity_") {
					sequences := 1
					if mode == "identity_limit" || mode == "identity_limit_summary" {
						sequences = 2
					}
					if counts["identity_sequence"] != sequences {
						t.Fatal("identity sequence was not observed")
					}
					wantCode := ""
					switch {
					case strings.HasPrefix(scenario, "identity_replay"), strings.HasPrefix(scenario, "identity_conflict_"):
						// Keep the child alive: lack of proof must reach the caller's
						// bounded deadline rather than an unrelated EOF cleanup error.
						wantCode = "deadline_exceeded"
					case strings.HasSuffix(scenario, "no_recall"):
						wantCode = "history_reply_not_recalled"
					case strings.HasPrefix(scenario, "identity_missing_"):
						// Generated DTOs reject omitted required IDs before delivery.
						wantCode = "sdk_observation_failed"
					case strings.HasPrefix(scenario, "identity_empty_"):
						wantCode = "assistant_identity_missing"
					case strings.HasPrefix(scenario, "identity_overflow"):
						wantCode = "assistant_identity_budget_exceeded"
						if counts["identity_overflow_sent"] != 1 || counts["identity_input_closed"] != 1 {
							t.Fatal("identity budget was not rejected before child EOF")
						}
					}
					if wantCode != "" {
						if err == nil || live.DescribeFailure(err).Code != wantCode || wantCode != "deadline_exceeded" && ctx.Err() != nil {
							t.Fatalf("identity failure did not preserve its safe category: want=%s evidence=%+v failure=%+v", wantCode, got, live.DescribeFailure(err))
						}
						recalled := got.FirstRecall
						if strings.HasPrefix(mode, "next_") {
							recalled = got.Recalled
							if !got.FirstCompleted || !got.FirstRecall || got.FirstReplyBytes != 64 {
								t.Fatal("continuation failure corrupted first-turn proof")
							}
						}
						if recalled {
							t.Fatal("replayed, ambiguous or invalid identity reported recall")
						}
						terminal := scenario != "identity_overflow" && !strings.HasSuffix(scenario, "_item") && !strings.HasPrefix(scenario, "identity_missing_")
						completed := got.FirstCompleted
						if strings.HasPrefix(mode, "next_") {
							completed = got.Completed
						}
						if completed != terminal || got.Stage == "history_recalled" {
							t.Fatal("identity failure lost or falsely reported terminal proof")
						}
					}
				}
				if historySuccess(mode) {
					calls, denied := 1, 0
					if mode == "repeated" {
						calls = 2
					}
					if mode == "tool_identity_limit" {
						calls = 4097
					}
					if mode == "next_denial" {
						denied = 1
					}
					if got.Stage != "history_recalled" || !got.Bound || !got.FirstCompleted || !got.FirstRecall || !got.Completed || !got.Recalled || got.FirstReplyBytes != 64 || got.ReplyBytes != 64 || got.ReasoningItems < 1 || got.EncryptedItems < 1 || got.ToolCalls != calls || got.CompletedTools < 1 || got.DeniedToolCalls != denied || counts["token_valid"] != calls || counts["denial_valid"] != denied {
						t.Fatal("missing complete history proof or causal tool validation")
					}
				} else if got.Stage == "history_recalled" {
					t.Fatal("incomplete history proof reported success")
				}
			})
		}
	}
}
