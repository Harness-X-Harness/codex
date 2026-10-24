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

// These in-memory HTTP cases protect the controlled fixture's stock prefix
// predicate. They do not replace either actual-binary child result scenario.
func testShippedChildInheritedHistory(t *testing.T) {
	message := func(role, text string) map[string]any {
		return map[string]any{"type": "message", "role": role, "content": []any{
			map[string]any{"type": "input_text", "text": text},
		}}
	}
	arguments, err := json.Marshal(map[string]any{"task_name": "live_child", "message": childHTTPTask})
	if err != nil {
		t.Fatal(err)
	}
	for _, tc := range []struct {
		name string
		edit func([]map[string]any) []map[string]any
		want string
	}{
		{name: "exact_inherited_pair"},
		{"missing_pair", func(in []map[string]any) []map[string]any {
			return append(in[:3], in[5])
		}, "child_spawn_replay_changed"},
		{"missing_call", func(in []map[string]any) []map[string]any {
			return append(in[:3], in[4:]...)
		}, "output_replay_binding"},
		{"missing_output", func(in []map[string]any) []map[string]any {
			return append(in[:4], in[5])
		}, "child_spawn_replay_changed"},
		{"extra_call", func(in []map[string]any) []map[string]any {
			return append(in, map[string]any{"type": "function_call", "call_id": "extra", "name": "fixture_spawn", "arguments": string(arguments)})
		}, "child_spawn_replay_changed"},
		{"extra_output", func(in []map[string]any) []map[string]any {
			return append(in, map[string]any{"type": "function_call_output", "call_id": "extra", "output": "aborted"})
		}, "output_replay_binding"},
		{"extra_pair", func(in []map[string]any) []map[string]any {
			return append(in,
				map[string]any{"type": "function_call", "call_id": "extra", "name": "fixture_spawn", "arguments": string(arguments)},
				map[string]any{"type": "function_call_output", "call_id": "extra", "output": "aborted"})
		}, "child_spawn_replay_changed"},
		{"duplicate_call", func(in []map[string]any) []map[string]any {
			return append(in, in[3])
		}, "call_replay_binding"},
		{"duplicate_output", func(in []map[string]any) []map[string]any {
			return append(in, in[4])
		}, "output_replay_binding"},
		{"wrong_call_id", func(in []map[string]any) []map[string]any {
			in[3]["call_id"], in[4]["call_id"] = "other_spawn", "other_spawn"
			return in
		}, "child_spawn_replay_changed"},
		{"missing_call_id", func(in []map[string]any) []map[string]any {
			delete(in[3], "call_id")
			return in
		}, "call_replay_binding"},
		{"wrong_name", func(in []map[string]any) []map[string]any {
			in[3]["name"] = "unrelated_tool"
			return in
		}, "child_spawn_replay_changed"},
		{"wrong_task_name", func(in []map[string]any) []map[string]any {
			in[3]["arguments"] = strings.Replace(string(arguments), "live_child", "other_child", 1)
			return in
		}, "child_spawn_replay_changed"},
		{"wrong_task_message", func(in []map[string]any) []map[string]any {
			in[3]["arguments"] = `{"task_name":"live_child","message":"different task"}`
			return in
		}, "child_spawn_replay_changed"},
		{"extra_spawn_argument", func(in []map[string]any) []map[string]any {
			in[3]["arguments"] = strings.TrimSuffix(string(arguments), "}") + `,"fork_turns":"none"}`
			return in
		}, "child_spawn_replay_changed"},
		{"malformed_arguments", func(in []map[string]any) []map[string]any {
			in[3]["arguments"] = "{"
			return in
		}, "child_spawn_replay_changed"},
		{"wrong_output_text", func(in []map[string]any) []map[string]any {
			in[4]["output"] = "completed"
			return in
		}, "child_spawn_replay_changed"},
		{"non_string_output", func(in []map[string]any) []map[string]any {
			in[4]["output"] = map[string]any{"text": "aborted"}
			return in
		}, "child_spawn_replay_changed"},
		{"wrong_output_binding", func(in []map[string]any) []map[string]any {
			in[4]["call_id"] = "other_spawn"
			return in
		}, "output_replay_binding"},
		{"output_before_call", func(in []map[string]any) []map[string]any {
			in[3], in[4] = in[4], in[3]
			return in
		}, "output_replay_binding"},
		{"output_not_adjacent", func(in []map[string]any) []map[string]any {
			return append(in[:4], message("developer", "intervening context"), in[4], in[5])
		}, "child_spawn_replay_changed"},
		{"pair_before_parent_task", func(in []map[string]any) []map[string]any {
			return []map[string]any{in[0], in[1], in[3], in[4], in[2], in[5]}
		}, "child_spawn_replay_changed"},
		{"new_task_before_pair", func(in []map[string]any) []map[string]any {
			return []map[string]any{in[0], in[1], in[2], in[5], in[3], in[4]}
		}, "child_task_binding"},
		{"missing_new_task", func(in []map[string]any) []map[string]any {
			return in[:5]
		}, "child_task_binding"},
		{"wrong_new_task_recipient", func(in []map[string]any) []map[string]any {
			in[5] = message("user", strings.Replace(childHTTPNewTask, "Task name: /root/live_child", "Task name: /root/other_child", 1))
			return in
		}, "child_task_binding"},
		{"wrong_new_task_sender", func(in []map[string]any) []map[string]any {
			in[5] = message("user", strings.Replace(childHTTPNewTask, "Sender: /root\n", "Sender: /root/other_child\n", 1))
			return in
		}, "child_task_binding"},
		{"wrong_new_task_payload", func(in []map[string]any) []map[string]any {
			in[5] = message("user", childHTTPNewTask+" Additional task.")
			return in
		}, "child_task_binding"},
		{"wrong_new_task_role", func(in []map[string]any) []map[string]any {
			in[5]["role"] = "assistant"
			return in
		}, "child_task_binding"},
		{"duplicate_new_task", func(in []map[string]any) []map[string]any {
			return append(in, in[5])
		}, "child_task_binding"},
		{"extra_new_task", func(in []map[string]any) []map[string]any {
			return append(in, message("user", childHTTPNewTask+" Additional task."))
		}, "child_task_binding"},
		{"missing_seed", func(in []map[string]any) []map[string]any {
			return in[1:]
		}, "seed_missing"},
		{"changed_seed", func(in []map[string]any) []map[string]any {
			in[0] = message("user", "Remember this bounded setup marker: "+strings.Repeat("02", 16)+". Reply briefly.")
			return in
		}, "seed_replay_changed"},
		{"seed_reply_out_of_order", func(in []map[string]any) []map[string]any {
			in[0], in[1] = in[1], in[0]
			return in
		}, "seed_prefix_missing"},
		{"duplicate_seed", func(in []map[string]any) []map[string]any {
			return append(in, in[0])
		}, "seed_replay_changed"},
		{"duplicate_seed_reply", func(in []map[string]any) []map[string]any {
			return append(in, in[1])
		}, "duplicate_seed_reply"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			integrationObservation(t)
			fixture := &shippedChildHTTP{model: "fixture-model"}
			input := []map[string]any{
				message("user", "Remember this bounded setup marker: "+strings.Repeat("01", 16)+". Reply briefly."),
				message("assistant", childHTTPSeedReply),
				message("user", "Delegate one bounded task to a child named live_child using inherited history."),
				{"type": "function_call", "call_id": "child_spawn", "name": "fixture_spawn", "arguments": string(arguments)},
				{"type": "function_call_output", "call_id": "child_spawn", "output": "aborted"},
				message("user", childHTTPNewTask),
			}
			seed := childHistoryFixtureRequest(t, fixture, "parent-thread", "high", input[:1])
			delegation := childHistoryFixtureRequest(t, fixture, "parent-thread", "xhigh", input[:3])
			if seed.Code != http.StatusOK || delegation.Code != http.StatusOK || !fixture.delegated || fixture.failure != "" {
				t.Fatalf("fixture setup failed: seed_status=%d delegation_status=%d delegated=%t failure=%s", seed.Code, delegation.Code, fixture.delegated, fixture.failure)
			}
			if tc.edit != nil {
				input = tc.edit(input)
			}
			response := childHistoryFixtureRequest(t, fixture, "child-thread", "xhigh", input)
			status := http.StatusBadRequest
			if tc.want == "" {
				status = http.StatusOK
				if fixture.childID == "" || fixture.nonce == "" || !strings.Contains(response.Body.String(), fixture.setupMarker+" "+fixture.nonce) {
					t.Fatal("valid inherited pair did not produce a fresh marker-and-nonce child reply")
				}
			} else if fixture.childID != "" || fixture.nonce != "" {
				t.Fatal("invalid inherited history reached child inference")
			}
			if response.Code != status || fixture.failure != tc.want || fixture.requests != 3 {
				t.Fatalf("history boundary: status=%d failure=%s requests=%d", response.Code, fixture.failure, fixture.requests)
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
