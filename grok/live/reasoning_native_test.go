package live_test

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"reflect"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

// The controlled backend observes actual product replay and tool execution;
// it supplies the answers and therefore does not establish real-provider recall.
func TestNativeReasoningHistoryFixture(t *testing.T) {
	binary := os.Getenv("GROK_LIVE_NATIVE_BIN")
	if binary == "" {
		t.Skip("activated by native runtime or explicit frozen-package verification")
	}
	subject := nativeSubject(t, binary)
	for _, tc := range []struct {
		name, model, effort string
	}{
		{"primary", providerfixture.PrimaryModel, "medium"},
		{"pinned", providerfixture.PinnedModel, "medium"},
		{"omitted-effort-control", providerfixture.PinnedModel, ""},
	} {
		t.Run(tc.name, func(t *testing.T) {
			fixture := &reasoningHTTP{model: tc.model, effort: tc.effort}
			backend := httptest.NewServer(fixture)
			defer backend.Close()
			ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
			defer cancel()
			run := live.ReasoningHistory
			if tc.effort == "" {
				run = live.ReasoningHistoryWithoutEffort
			}
			got, err := run(ctx, live.Options{Subject: subject, Model: tc.model, BaseURL: backend.URL + "/v1", APIKey: "fixture-key"})
			fixture.mu.Lock()
			defer fixture.mu.Unlock()
			// Evidence and structural failure markers never contain token, raw
			// prompts, identifiers, private reasoning or encrypted content.
			encoded, _ := json.Marshal(got)
			failure := live.FailureInfo{}
			if err != nil {
				failure = live.DescribeFailure(err)
			}
			t.Logf("controlled reasoning: evidence=%s requests=%d paired_result=%t encrypted_replay=%t continuation=%t wire_failure=%s failure=%+v",
				encoded, fixture.requests, fixture.token != "", fixture.replayed, fixture.continued, fixture.failure, failure)
			if err != nil || fixture.failure != "" || fixture.requests != 3 || !fixture.replayed || !fixture.continued {
				t.Fatal("native reasoning/tool/history composition was not proven")
			}
			if got.ReasoningItems < 1 || got.EncryptedItems < 1 || got.CompletedTools < 1 || got.FirstReplyBytes != 64 || got.ReplyBytes != 64 {
				t.Fatal("completed encrypted reasoning, paired tool and final recall evidence missing")
			}
			got.ObservedAt = ""
			want := live.Evidence{SHA256: subject.SHA256, SourceSHA: subject.SourceSHA, HarnessSHA: subject.HarnessSHA,
				Target: subject.Target, Environment: subject.Environment, Model: tc.model, Stage: "history_recalled",
				Processes: 1, Initializations: 1, Threads: 1, Turns: 2, Bound: true, Completed: true,
				FirstCompleted: true, FirstRecall: true, Recalled: true, FirstReplyBytes: 64, ReplyBytes: 64,
				ToolCalls: 1, CompletedTools: got.CompletedTools, ReasoningItems: got.ReasoningItems, EncryptedItems: got.EncryptedItems}
			if got != want {
				t.Fatal("native two-turn evidence changed")
			}
		})
	}
}

const reasoningHTTPCipher = "opaque-controlled-history-replay"
const reasoningHTTPPrivate = "CONTROLLED_PRIVATE_REASONING"
const reasoningHTTPCall = "controlled_history_call"

type reasoningHTTP struct {
	mu                                       sync.Mutex
	model, effort, threadID, toolName, token string
	failure                                  string
	requests                                 int
	replayed, continued                      bool
	output                                   json.RawMessage
}

func (fixture *reasoningHTTP) ServeHTTP(writer http.ResponseWriter, request *http.Request) {
	fixture.mu.Lock()
	defer fixture.mu.Unlock()
	defer request.Body.Close()
	fixture.requests++
	fail := func(reason string) {
		if fixture.failure == "" {
			fixture.failure = reason
		}
		http.Error(writer, reason, http.StatusBadRequest)
	}
	var body struct {
		Model     string
		Stream    bool
		Store     *bool
		Include   []string
		Reasoning map[string]json.RawMessage
		Tools     []json.RawMessage
		Input     []json.RawMessage
	}
	data, err := io.ReadAll(io.LimitReader(request.Body, (1<<20)+1))
	threadID := request.Header.Get("thread-id")
	if err != nil || len(data) > 1<<20 || json.Unmarshal(data, &body) != nil ||
		request.Method != "POST" || request.URL.Path != "/v1/responses" || request.Header.Get("Authorization") != "Bearer fixture-key" ||
		threadID == "" || body.Model != fixture.model || !body.Stream || body.Store == nil || *body.Store ||
		len(body.Input) > 128 || len(body.Tools) > 16 || fixture.requests > 3 {
		fail("request_contract")
		return
	}
	include, effort := false, ""
	for _, field := range body.Include {
		include = include || field == "reasoning.encrypted_content"
	}
	rawEffort, hasEffort := body.Reasoning["effort"]
	if !include || (fixture.effort == "" && hasEffort) ||
		(fixture.effort != "" && (!hasEffort || json.Unmarshal(rawEffort, &effort) != nil || effort != fixture.effort)) {
		fail("reasoning_request_changed")
		return
	}
	name, ok := reasoningHTTPDeclaration(body.Tools)
	if !ok || (fixture.toolName != "" && name != fixture.toolName) || (fixture.threadID != "" && threadID != fixture.threadID) {
		fail("tool_or_thread_changed")
		return
	}
	fixture.toolName, fixture.threadID = name, threadID
	calls, outputs, encrypted, firstPrompts, continuations, firstFinals := 0, 0, 0, 0, 0, 0
	reasoningAt, callAt, outputAt, finalAt, continuationAt := -1, -1, -1, -1, -1
	for index, raw := range body.Input {
		var item struct {
			Type, Role, Name, Arguments string
			CallID                      string `json:"call_id"`
			EncryptedContent            string `json:"encrypted_content"`
			Output                      json.RawMessage
			Content                     []struct{ Type, Text string }
		}
		if json.Unmarshal(raw, &item) != nil {
			fail("invalid_history_item")
			return
		}
		switch item.Type {
		case "reasoning":
			encrypted++
			reasoningAt = index
			var fields map[string]json.RawMessage
			_ = json.Unmarshal(raw, &fields)
			if item.EncryptedContent != reasoningHTTPCipher || string(fields["summary"]) != "[]" || fields["content"] != nil {
				fail("encrypted_history_changed")
				return
			}
		case "function_call":
			calls++
			callAt = index
			var arguments map[string]any
			if item.CallID != reasoningHTTPCall || item.Name != fixture.toolName || json.Unmarshal([]byte(item.Arguments), &arguments) != nil || len(arguments) != 0 {
				fail("tool_call_replay_changed")
				return
			}
		case "function_call_output":
			outputs++
			outputAt = index
			var token string
			if item.CallID != reasoningHTTPCall || json.Unmarshal(item.Output, &token) != nil || !historyNonce.MatchString(token) {
				fail("tool_result_binding")
				return
			}
			if fixture.token == "" {
				fixture.token = token
				fixture.output = append(json.RawMessage(nil), item.Output...)
			} else if fixture.token != token || !bytes.Equal(fixture.output, item.Output) {
				fail("tool_result_replay_changed")
				return
			}
		case "message":
			for _, part := range item.Content {
				if item.Role == "user" && fixture.token != "" && strings.Contains(part.Text, fixture.token) {
					fail("user_secret_inserted")
					return
				}
				if item.Role == "user" && strings.HasPrefix(part.Text, "Call grok_history_probe,") {
					firstPrompts++
				}
				if item.Role == "user" && strings.HasPrefix(part.Text, "Recall the exact token returned by the tool in the previous turn") {
					continuations++
					continuationAt = index
					if fixture.token == "" || strings.Contains(part.Text, fixture.token) || strings.Contains(part.Text, reasoningHTTPCipher) {
						fail("continuation_secret_inserted")
						return
					}
				}
				if item.Role == "assistant" && fixture.token != "" && part.Text == fixture.token {
					firstFinals++
					finalAt = index
				}
			}
		}
	}
	if bytes.Contains(data, []byte(reasoningHTTPPrivate)) || firstPrompts != 1 || calls > 1 || outputs > 1 || encrypted > 1 {
		fail("history_contract")
		return
	}
	if fixture.requests == 1 {
		if calls != 0 || outputs != 0 || encrypted != 0 || continuations != 0 || firstFinals != 0 {
			fail("first_turn_input")
			return
		}
	} else if calls != 1 || outputs != 1 || encrypted != 1 || continuations != fixture.requests-2 || firstFinals != fixture.requests-2 {
		fail("history_prefix_missing")
		return
	}
	if fixture.requests > 1 && (reasoningAt >= callAt || callAt >= outputAt ||
		(fixture.requests == 3 && (outputAt >= finalAt || finalAt >= continuationAt))) {
		fail("history_order_changed")
		return
	}
	fixture.replayed = fixture.replayed || fixture.requests == 2
	fixture.continued = fixture.requests == 3
	writer.Header().Set("Content-Type", "text/event-stream")
	emit := func(event any) {
		encoded, _ := json.Marshal(event)
		_, _ = fmt.Fprintf(writer, "data: %s\n\n", encoded)
	}
	responseID := fmt.Sprintf("controlled_history_%d", fixture.requests)
	emit(map[string]any{"type": "response.created", "response": map[string]any{"id": responseID}})
	if fixture.requests == 1 {
		reasoning := map[string]any{"type": "reasoning", "id": "controlled_reasoning", "summary": []any{}, "encrypted_content": reasoningHTTPCipher,
			"content": []any{map[string]any{"type": "reasoning_text", "text": reasoningHTTPPrivate}}}
		emit(map[string]any{"type": "response.output_item.added", "output_index": 0, "item": reasoning})
		emit(map[string]any{"type": "response.output_item.done", "output_index": 0, "item": reasoning})
		call := map[string]any{"type": "function_call", "id": "controlled_call_item", "call_id": reasoningHTTPCall, "name": fixture.toolName, "arguments": ""}
		emit(map[string]any{"type": "response.output_item.added", "output_index": 1, "item": call})
		emit(map[string]any{"type": "response.function_call_arguments.delta", "output_index": 1, "delta": "{}"})
		call["arguments"], call["status"] = "{}", "completed"
		emit(map[string]any{"type": "response.output_item.done", "output_index": 1, "item": call})
	} else {
		message := map[string]any{"type": "message", "id": responseID + "_message", "role": "assistant", "phase": "final_answer", "content": []any{}}
		emit(map[string]any{"type": "response.output_item.added", "output_index": 0, "item": message})
		emit(map[string]any{"type": "response.output_text.delta", "output_index": 0, "delta": fixture.token})
		message["status"], message["content"] = "completed", []any{map[string]any{"type": "output_text", "text": fixture.token}}
		emit(map[string]any{"type": "response.output_item.done", "output_index": 0, "item": message})
	}
	emit(map[string]any{"type": "response.completed", "response": map[string]any{"id": responseID}})
}

func reasoningHTTPDeclaration(tools []json.RawMessage) (string, bool) {
	const projected = "local___grok_history_probe_5c6dab5ce2f3142e"
	var name string
	hosted := false
	for _, raw := range tools {
		var tool struct {
			Type, Name string
			Parameters map[string]any
		}
		if json.Unmarshal(raw, &tool) != nil {
			return "", false
		}
		switch tool.Type {
		case "x_search":
			var fields map[string]json.RawMessage
			if hosted || json.Unmarshal(raw, &fields) != nil || len(fields) != 1 {
				return "", false
			}
			hosted = true
		case "function":
			if name != "" || tool.Name != projected || !reflect.DeepEqual(tool.Parameters,
				map[string]any{"type": "object", "properties": map[string]any{}, "additionalProperties": false}) {
				return "", false
			}
			name = tool.Name
		default:
			return "", false
		}
	}
	return name, name != ""
}
