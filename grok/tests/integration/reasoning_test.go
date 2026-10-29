//go:build integration

package integration_test

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/integrationfixture"
	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

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
		Text      struct {
			Format struct {
				Type   string
				Strict bool
				Schema json.RawMessage
			}
		}
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
	var actualSchema, expectedSchema any
	_ = json.Unmarshal(body.Text.Format.Schema, &actualSchema)
	_ = json.Unmarshal([]byte(reasoningAmountSchema), &expectedSchema)
	if body.Text.Format.Type != "json_schema" || !body.Text.Format.Strict || !reflect.DeepEqual(actualSchema, expectedSchema) {
		fail("output_schema_changed")
		return
	}
	include := false
	for _, field := range body.Include {
		include = include || field == "reasoning.encrypted_content"
	}
	_, hasEffort := body.Reasoning["effort"]
	if !include || hasEffort {
		fail("reasoning_request_changed")
		return
	}
	name, ok := integrationfixture.HistoryProbeDeclaration(body.Tools)
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
			var summary []json.RawMessage
			if item.EncryptedContent != reasoningHTTPCipher || json.Unmarshal(fields["summary"], &summary) != nil ||
				summary == nil || len(summary) != 0 || fields["content"] != nil {
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
			var priceResult string
			if item.CallID != reasoningHTTPCall || json.Unmarshal(item.Output, &priceResult) != nil || !validReasoningPrice(priceResult) {
				fail("tool_result_binding")
				return
			}
			if fixture.priceResult == "" {
				fixture.priceResult = priceResult
			} else if fixture.priceResult != priceResult {
				fail("tool_result_replay_changed")
				return
			}
		case "message":
			for _, part := range item.Content {
				if item.Role == "user" && fixture.priceResult != "" && strings.Contains(part.Text, fixture.priceResult) {
					fail("user_secret_inserted")
					return
				}
				if item.Role == "user" && part.Text == reasoningFirstPrompt {
					firstPrompts++
				}
				if item.Role == "user" && part.Text == reasoningContinuationPrompt {
					continuations++
					continuationAt = index
					if fixture.priceResult == "" || strings.Contains(part.Text, fixture.priceResult) || strings.Contains(part.Text, reasoningHTTPCipher) {
						fail("continuation_secret_inserted")
						return
					}
				}
				if item.Role == "assistant" && fixture.priceResult != "" && part.Text == reasoningAmountReply(fixture.priceResult, 2) {
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
		emit(map[string]any{"type": "response.output_text.delta", "output_index": 0, "delta": reasoningAmountReply(fixture.priceResult, fixture.requests)})
		message["status"], message["content"] = "completed", []any{map[string]any{"type": "output_text", "text": reasoningAmountReply(fixture.priceResult, fixture.requests)}}
		emit(map[string]any{"type": "response.output_item.done", "output_index": 0, "item": message})
	}
	emit(map[string]any{"type": "response.completed", "response": map[string]any{"id": responseID}})
}

type reasoningHTTP struct {
	mu                                     sync.Mutex
	model, threadID, toolName, priceResult string
	failure                                string
	requests                               int
	replayed, continued                    bool
}

const reasoningHTTPCall = "controlled_history_call"

const reasoningHTTPCipher = "opaque-controlled-history-replay"

const reasoningHTTPPrivate = "CONTROLLED_PRIVATE_REASONING"

// The controlled backend observes actual product replay and tool execution;
// it supplies the answers and therefore does not establish real-provider recall.
func TestReasoningHistoryFixture(t *testing.T) {
	t.Run("replay_semantics", testReasoningFixtureReplaySemantics)
	for _, tc := range []struct {
		name, model string
	}{
		{"primary", providerfixture.PrimaryModel},
		{"pinned", providerfixture.PinnedModel},
	} {
		t.Run(tc.name, func(t *testing.T) {
			integrationObservation(t)
			subject := nativeSubject(t)
			fixture := &reasoningHTTP{model: tc.model}
			backend := httptest.NewServer(fixture)
			defer backend.Close()
			ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
			defer cancel()
			got, err := live.ReasoningHistory(ctx, live.Options{Subject: subject, Model: tc.model, BaseURL: backend.URL + "/v1", APIKey: "fixture-key"})
			fixture.mu.Lock()
			defer fixture.mu.Unlock()
			// Evidence and structural failure markers never contain priceResult, raw
			// prompts, identifiers, private reasoning or encrypted content.
			encoded, _ := json.Marshal(got)
			failure := live.FailureInfo{}
			if err != nil {
				failure = live.DescribeFailure(err)
			}
			t.Logf("controlled reasoning: evidence=%s requests=%d paired_result=%t encrypted_replay=%t continuation=%t wire_failure=%s failure=%+v",
				encoded, fixture.requests, fixture.priceResult != "", fixture.replayed, fixture.continued, fixture.failure, failure)
			if err != nil || fixture.failure != "" || fixture.requests != 3 || !fixture.replayed || !fixture.continued {
				t.Fatal("native reasoning/tool/history composition was not proven")
			}
			if got.ReasoningItems < 1 || got.EncryptedItems < 1 || got.CompletedTools < 1 || got.FirstReplyBytes != 13 || got.ReplyBytes != 13 {
				t.Fatal("completed encrypted reasoning, paired tool and final recall evidence missing")
			}
			if got.LocalUnverified != subject.LocalUnverified || got.SHA256 != subject.SHA256 || got.SourceSHA != subject.SourceSHA || got.HarnessSHA != subject.HarnessSHA ||
				got.Target != subject.Target || got.Environment != subject.Environment || got.Model != tc.model ||
				got.Stage != "history_recalled" || !got.Bound || !got.Completed || !got.FirstCompleted ||
				!got.FirstRecall || !got.Recalled || got.CleanupPending {
				t.Fatal("native two-turn identity, completion or recall evidence missing")
			}
		})
	}
}

const reasoningFirstPrompt = "Check the shop’s current apple price using the price lookup, then calculate the cost of two kilograms."
const reasoningContinuationPrompt = "Using the price we already checked, how much would three kilograms of apples cost? Do not look up the price again."

const reasoningAmountSchema = `{"type":"object","properties":{"amount":{"type":["number","null"]}},"required":["amount"],"additionalProperties":false}`

func validReasoningPrice(value string) bool {
	var result struct {
		UnitPrice int `json:"unit_price_yuan"`
	}
	return json.Unmarshal([]byte(value), &result) == nil && result.UnitPrice >= 5 && result.UnitPrice <= 19 && value == fmt.Sprintf(`{"unit_price_yuan":%d}`, result.UnitPrice)
}

func reasoningAmountReply(value string, kilograms int) string {
	var result struct {
		UnitPrice int `json:"unit_price_yuan"`
	}
	_ = json.Unmarshal([]byte(value), &result)
	data, _ := json.Marshal(map[string]int{"amount": result.UnitPrice * kilograms})
	return string(data)
}
