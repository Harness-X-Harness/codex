//go:build integration

package integration_test

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

// These cases test task/context, not a unique stock history serialization.
// The actual-binary cases separately own child execution and parent delivery.
func testShippedChildInheritedHistory(t *testing.T) {
	message := func(role, text string) map[string]any {
		return map[string]any{"type": "message", "role": role, "content": []any{map[string]any{"type": "input_text", "text": text}}}
	}
	for _, tc := range []struct {
		name string
		edit func([]map[string]any) []map[string]any
		want string
	}{
		{name: "stock_inherited_pair"},
		{"no_historical_tools", func(in []map[string]any) []map[string]any { return append(in[:3], in[5]) }, ""},
		{"equivalent_context_presentation", func(in []map[string]any) []map[string]any {
			in[0]["id"] = "reconstructed-seed-item"
			in[4]["output"] = "historical operation interrupted"
			in[5] = message("user", childHTTPTask)
			return []map[string]any{in[0], in[1], in[2], in[3], message("developer", "context"), in[4], in[5]}
		}, ""},
		{"wrong_output_binding", func(in []map[string]any) []map[string]any { in[4]["call_id"] = "unrelated-call"; return in }, "output_replay_binding"},
		{"missing_task", func(in []map[string]any) []map[string]any { return in[:5] }, "child_task_binding"},
		{"changed_seed", func(in []map[string]any) []map[string]any {
			in[0] = message("user", "Remember this bounded setup marker: "+strings.Repeat("02", 16)+". Reply briefly.")
			return in
		}, "seed_replay_changed"},
		{"reordered_seed", func(in []map[string]any) []map[string]any { in[0], in[1] = in[1], in[0]; return in }, "seed_prefix_missing"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			integrationObservation(t)
			fixture := &shippedChildHTTP{model: "fixture-model", seedEffort: "high"}
			args, _ := json.Marshal(map[string]any{"task_name": "live_child", "message": childHTTPTask})
			input := []map[string]any{
				message("user", "Remember this bounded setup marker: "+strings.Repeat("01", 16)+". Reply briefly."),
				message("assistant", childHTTPSeedReply),
				message("user", "Delegate one bounded task to a child named live_child using inherited history."),
				{"type": "function_call", "call_id": "child_spawn", "name": "fixture_spawn", "arguments": string(args)},
				{"type": "function_call_output", "call_id": "child_spawn", "output": "aborted"},
				message("user", childHTTPNewTask),
			}
			seed := childHistoryFixtureRequest(t, fixture, "parent-thread", "high", input[:1])
			delegation := childHistoryFixtureRequest(t, fixture, "parent-thread", "xhigh", input[:3])
			if seed.Code != http.StatusOK || delegation.Code != http.StatusOK || !fixture.delegated || fixture.failure != "" {
				t.Fatal("fixture setup did not reach delegation")
			}
			if tc.edit != nil {
				input = tc.edit(input)
			}
			response := childHistoryFixtureRequest(t, fixture, "child-thread", "xhigh", input)
			status := http.StatusBadRequest
			if tc.want == "" {
				status = http.StatusOK
				if fixture.childID == "" || fixture.nonce == "" || !strings.Contains(response.Body.String(), fixture.setupMarker+" "+fixture.nonce) {
					t.Fatal("valid task/context did not produce the fresh child witness")
				}
			} else if fixture.childID != "" || fixture.nonce != "" {
				t.Fatal("invalid task/context produced the child witness")
			}
			if response.Code != status || fixture.failure != tc.want {
				t.Fatalf("context boundary: status=%d failure=%s", response.Code, fixture.failure)
			}
		})
	}
}

func childHistoryFixtureRequest(t *testing.T, fixture *shippedChildHTTP, thread, effort string, input []map[string]any) *httptest.ResponseRecorder {
	t.Helper()
	tools := []any{
		map[string]any{"type": "function", "name": "fixture_spawn", "description": "Call this function directly to invoke `collaboration.spawn_agent`.",
			"parameters": map[string]any{"type": "object", "additionalProperties": false, "required": []string{"task_name", "message"},
				"properties": map[string]any{"task_name": map[string]any{"type": "string"}, "message": map[string]any{"type": "string"}, "fork_turns": map[string]any{"type": "string"}}}},
		map[string]any{"type": "function", "name": "fixture_wait", "description": "Call this function directly to invoke `collaboration.wait_agent`.",
			"parameters": map[string]any{"type": "object", "additionalProperties": false,
				"properties": map[string]any{"timeout_ms": map[string]any{"type": "number"}}}},
	}
	body, err := json.Marshal(map[string]any{"model": fixture.model, "tools": tools, "input": input,
		"stream": true, "store": false, "reasoning": map[string]any{"effort": effort}})
	if err != nil {
		t.Fatal(err)
	}
	request := httptest.NewRequest(http.MethodPost, "/v1/responses", bytes.NewReader(body))
	request.Header.Set("Authorization", "Bearer fixture-key")
	request.Header.Set("thread-id", thread)
	response := httptest.NewRecorder()
	fixture.ServeHTTP(response, request)
	return response
}

func testShippedChildDeliveredResult(t *testing.T) {
	const marker = "00112233445566778899aabbccddeeff"
	const nonce = "01234567-89ab-4cde-8fab-0123456789ab"
	const header = "Message Type: FINAL_ANSWER\nTask name: /root\nSender: /root/live_child\nPayload:\n"
	valid := header + "The inherited marker is " + marker + ". My result is " + nonce + "."
	for _, tc := range []struct {
		name, text string
		want       bool
	}{
		{"result_with_prose", valid, true},
		{"wrong_result", strings.Replace(valid, nonce, "another result", 1), false},
		{"missing_marker", strings.Replace(valid, marker, "", 1), false},
		{"non_child_source", strings.Replace(valid, "Sender: /root/live_child", "Sender: /root", 1), false},
		{"wrong_recipient", strings.Replace(valid, "Task name: /root\n", "Task name: /root/other\n", 1), false},
		{"plain_user_quotation", marker + " " + nonce, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			integrationObservation(t)
			if childHTTPDelivered(tc.text, "/root/live_child", marker, nonce) != tc.want {
				t.Fatal("child result identity/value boundary changed")
			}
		})
	}
}
