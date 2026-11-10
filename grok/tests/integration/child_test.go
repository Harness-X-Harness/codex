//go:build integration

package integration_test

import (
	"bytes"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"reflect"
	"strings"
	"sync"
	"testing"

	"github.com/Harness-X-Harness/codex/grok/internal/integrationfixture"
)

func childHTTPCall(writer http.ResponseWriter, id, name string, arguments any, knownParentText ...string) {
	writer.Header().Set("Content-Type", "text/event-stream")
	childHTTPEvent(writer, map[string]any{"type": "response.created", "response": map[string]any{"id": id}})
	index := 0
	for _, text := range knownParentText {
		childHTTPMessage(writer, id+"_known", text, index)
		index++
	}
	args, _ := json.Marshal(arguments)
	item := map[string]any{"type": "function_call", "id": id + "_item", "call_id": id, "name": name, "arguments": ""}
	childHTTPEvent(writer, map[string]any{"type": "response.output_item.added", "output_index": index, "item": item})
	childHTTPEvent(writer, map[string]any{"type": "response.function_call_arguments.delta", "output_index": index, "delta": string(args)})
	item["arguments"], item["status"] = string(args), "completed"
	childHTTPEvent(writer, map[string]any{"type": "response.output_item.done", "output_index": index, "item": item})
	childHTTPEvent(writer, map[string]any{"type": "response.completed", "response": map[string]any{"id": id}})
}

func childHTTPEvent(writer http.ResponseWriter, event any) {
	encoded, _ := json.Marshal(event)
	_, _ = fmt.Fprintf(writer, "data: %s\n\n", encoded)
}

type childHTTPItem struct {
	Type, ID, Role, Name string
	CallID               string `json:"call_id"`
	Arguments            string
	Output               json.RawMessage
	Content              []struct{ Type, Text string }
}

func childHTTPMessage(writer http.ResponseWriter, id, text string, index int) {
	item := map[string]any{"type": "message", "id": id + "_message", "role": "assistant", "phase": "final_answer", "content": []any{}}
	childHTTPEvent(writer, map[string]any{"type": "response.output_item.added", "output_index": index, "item": item})
	childHTTPEvent(writer, map[string]any{"type": "response.output_text.delta", "output_index": index, "delta": text})
	item["status"], item["content"] = "completed", []any{map[string]any{"type": "output_text", "text": text}}
	childHTTPEvent(writer, map[string]any{"type": "response.output_item.done", "output_index": index, "item": item})
}

func childHTTPNonce() (string, error) {
	var value [16]byte
	if _, err := rand.Read(value[:]); err != nil {
		return "", err
	}
	value[6], value[8] = value[6]&0x0f|0x40, value[8]&0x3f|0x80
	return fmt.Sprintf("%x-%x-%x-%x-%x", value[:4], value[4:6], value[6:8], value[8:10], value[10:]), nil
}

func childHTTPReply(writer http.ResponseWriter, id, text string) {
	writer.Header().Set("Content-Type", "text/event-stream")
	childHTTPEvent(writer, map[string]any{"type": "response.created", "response": map[string]any{"id": id}})
	childHTTPMessage(writer, id, text, 0)
	childHTTPEvent(writer, map[string]any{"type": "response.completed", "response": map[string]any{"id": id}})
}

const childHTTPParentSchema = `{"type":"object","properties":{"child_nonce":{"type":["string","null"]}},"required":["child_nonce"],"additionalProperties":false}`

const childHTTPSeedReply = "seed accepted"

const childHTTPTask = "Recall the setup marker from your inherited history and create a fresh UUID v4. Include the marker and your UUID in your answer. Do not ask the parent to supply either value."

const childHTTPNewTask = "Message Type: NEW_TASK\nTask name: /root/live_child\nSender: /root\nPayload:\n" + childHTTPTask

func (fixture *shippedChildHTTP) ServeHTTP(writer http.ResponseWriter, request *http.Request) {
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
		Model  string
		Tools  []json.RawMessage
		Input  []json.RawMessage
		Stream bool
		Store  *bool
		Text   struct {
			Format struct {
				Type   string
				Strict bool
				Schema json.RawMessage
			}
		}
		Reasoning struct{ Effort string }
	}
	data, err := io.ReadAll(io.LimitReader(request.Body, (1<<20)+1))
	threadID := request.Header.Get("thread-id")
	if err != nil || len(data) > 1<<20 || json.Unmarshal(data, &body) != nil ||
		request.Method != "POST" || request.URL.Path != "/v1/responses" ||
		request.Header.Get("Authorization") != "Bearer fixture-key" || threadID == "" ||
		body.Model != fixture.model || !body.Stream || body.Store == nil || *body.Store || len(body.Input) > 128 || fixture.requests > 5 {
		fail("request_contract")
		return
	}
	if fixture.parentID != "" && threadID == fixture.parentID {
		var gotSchema, wantSchema any
		if json.Unmarshal(body.Text.Format.Schema, &gotSchema) != nil || json.Unmarshal([]byte(childHTTPParentSchema), &wantSchema) != nil ||
			body.Text.Format.Type != "json_schema" || !body.Text.Format.Strict || !reflect.DeepEqual(gotSchema, wantSchema) {
			fail("parent_result_schema")
			return
		}
	} else if body.Text.Format.Type == "json_schema" || len(body.Text.Format.Schema) != 0 {
		// The seed and child are natural tasks; the parent-only final format
		// must not silently become a child output constraint.
		fail("unexpected_child_or_seed_schema")
		return
	}
	// Ultra is the stock local alias for xhigh on the wire.
	effort := "xhigh"
	if fixture.parentID == "" {
		effort = fixture.seedEffort
	}
	if body.Reasoning.Effort != effort {
		fail("reasoning_effort_changed")
		return
	}
	spawn, err := integrationfixture.ChildDeclaration(body.Tools, "spawn_agent")
	if err != nil {
		fail("unsupported_spawn_declaration")
		return
	}
	wait, err := integrationfixture.ChildDeclaration(body.Tools, "wait_agent")
	if err != nil {
		fail("unsupported_wait_declaration")
		return
	}
	if fixture.spawn == "" {
		fixture.spawn, fixture.wait = spawn, wait
	} else if spawn != fixture.spawn || wait != fixture.wait {
		fail("declaration_identity_changed")
		return
	}
	var userText []string
	seedAt, replyAt, taskAt := -1, -1, -1
	newTaskAt := -1
	calls, outputs := make(map[string]childHTTPItem), make(map[string]childHTTPItem)
	for index, raw := range body.Input {
		var item childHTTPItem
		if json.Unmarshal(raw, &item) != nil {
			fail("invalid_input")
			return
		}
		switch item.Type {
		case "message":
			for _, part := range item.Content {
				if part.Type != "input_text" && part.Type != "output_text" {
					fail("unsupported_message_content")
					return
				}
				if item.Role == "user" {
					userText = append(userText, part.Text)
				}
				if item.Role == "user" && strings.HasPrefix(part.Text, "Remember this bounded setup marker: ") {
					if seedAt >= 0 || (fixture.seed != "" && part.Text != fixture.seed) {
						fail("seed_replay_changed")
						return
					}
					fixture.seed, seedAt = part.Text, index
					marker := strings.TrimPrefix(strings.SplitN(part.Text, ". ", 2)[0], "Remember this bounded setup marker: ")
					decoded, decodeErr := hex.DecodeString(marker)
					if decodeErr != nil || len(decoded) != 16 {
						fail("setup_marker_invalid")
						return
					}
					fixture.setupMarker = marker
				}
				if item.Role == "assistant" && part.Text == childHTTPSeedReply {
					if replyAt >= 0 {
						fail("duplicate_seed_reply")
						return
					}
					replyAt = index
				}
				if item.Role == "user" && strings.HasPrefix(part.Text, "Delegate one bounded task to a child named live_child ") {
					taskAt = index
				}
				if item.Role == "user" && strings.Contains(part.Text, childHTTPTask) &&
					!strings.HasPrefix(part.Text, "Delegate one bounded task to a child named live_child ") {
					newTaskAt = index
				}
			}
		case "function_call":
			if item.CallID == "" || calls[item.CallID].CallID != "" {
				fail("call_replay_binding")
				return
			}
			calls[item.CallID] = item
		case "function_call_output":
			if item.CallID == "" || outputs[item.CallID].CallID != "" || calls[item.CallID].CallID == "" {
				fail("output_replay_binding")
				return
			}
			outputs[item.CallID] = item
		default:
			fail("unsupported_input_kind")
			return
		}
	}
	if seedAt < 0 {
		fail("seed_missing")
		return
	}
	if fixture.parentID == "" {
		if replyAt >= 0 || taskAt >= 0 || len(calls) != 0 {
			fail("initial_seed_input")
			return
		}
		fixture.parentID = threadID
		childHTTPReply(writer, "seed", childHTTPSeedReply)
		return
	}
	if replyAt <= seedAt || taskAt <= replyAt {
		fail("seed_prefix_missing")
		return
	}
	if threadID != fixture.parentID {
		if fixture.childID != "" || !fixture.delegated {
			fail("child_input_binding")
			return
		}
		// The child must receive the controlled task after the inherited seed.
		// Historical tool records are not the delegation outcome: stock may
		// retain/normalize them without changing the task or its causal result.
		if newTaskAt <= taskAt {
			fail("child_task_binding")
			return
		}
		fixture.childID = threadID
		if !fixture.parentSupplied {
			fixture.nonce, err = childHTTPNonce()
			if err != nil {
				fail("nonce_unavailable")
				return
			}
		}
		// The negative deliberately supplies the nonce through inherited parent
		// final text, never through NEW_TASK. Require that actual wire witness.
		if fixture.nonce == "" || bytes.Contains(data, []byte(fixture.nonce)) != fixture.parentSupplied {
			fail("nonce_provenance_changed")
			return
		}
		childHTTPReply(writer, "child", fixture.setupMarker+" "+fixture.nonce)
		return
	}
	if !fixture.delegated {
		if len(calls) != 0 {
			fail("delegation_input")
			return
		}
		fixture.delegated = true
		if fixture.parentSupplied {
			fixture.nonce, err = childHTTPNonce()
			if err != nil {
				fail("nonce_unavailable")
				return
			}
			knownResult, _ := json.Marshal(map[string]any{"child_nonce": fixture.nonce})
			childHTTPCall(writer, "child_spawn", fixture.spawn, map[string]any{"task_name": "live_child", "message": childHTTPTask}, string(knownResult))
		} else {
			childHTTPCall(writer, "child_spawn", fixture.spawn, map[string]any{"task_name": "live_child", "message": childHTTPTask})
		}
		return
	}
	// Separate responses ensure spawn has completed before wait can acquire the
	// stock nonparallel tool lock. A child that already replied needs no wait.
	expected := 1
	if fixture.waited {
		expected++
	}
	spawnCall, waitCall := calls["child_spawn"], calls["child_wait"]
	var spawnArgs map[string]any
	var spawnText string
	var spawnResult struct {
		TaskName string `json:"task_name"`
	}
	if fixture.continued || len(calls) != expected || len(outputs) != expected ||
		spawnCall.Name != fixture.spawn ||
		json.Unmarshal([]byte(spawnCall.Arguments), &spawnArgs) != nil ||
		!reflect.DeepEqual(spawnArgs, map[string]any{"task_name": "live_child", "message": childHTTPTask}) ||
		json.Unmarshal(outputs["child_spawn"].Output, &spawnText) != nil ||
		json.Unmarshal([]byte(spawnText), &spawnResult) != nil || spawnResult.TaskName != "/root/live_child" {
		fail("spawn_replay_changed")
		return
	}
	if fixture.waited {
		var waitArgs map[string]any
		var waitText string
		var result struct {
			TimedOut *bool `json:"timed_out"`
		}
		if waitCall.Name != fixture.wait || json.Unmarshal([]byte(waitCall.Arguments), &waitArgs) != nil ||
			!reflect.DeepEqual(waitArgs, map[string]any{"timeout_ms": float64(10000)}) ||
			json.Unmarshal(outputs["child_wait"].Output, &waitText) != nil ||
			json.Unmarshal([]byte(waitText), &result) != nil || result.TimedOut == nil || *result.TimedOut {
			fail("wait_replay_changed")
			return
		}
	}
	delivered := false
	for _, text := range userText {
		delivered = delivered || childHTTPDelivered(text, spawnResult.TaskName, fixture.setupMarker, fixture.nonce)
	}
	if fixture.childID == "" || fixture.nonce == "" || !delivered {
		if fixture.waited {
			fail("parent_completion_missing")
			return
		}
		fixture.waited = true
		childHTTPCall(writer, "child_wait", fixture.wait, map[string]any{"timeout_ms": 10000})
		return
	}
	fixture.continued = true
	parentResult, _ := json.Marshal(map[string]any{"child_nonce": fixture.nonce})
	childHTTPReply(writer, "parent", string(parentResult))
}

// The public agent-message projection owns these routing fields
// (protocol/src/protocol.rs). Bind sender/recipient and result values, not the
// exact header ordering or surrounding payload prose.
func childHTTPDelivered(text, sender, marker, nonce string) bool {
	header, payload, ok := strings.Cut(text, "\nPayload:\n")
	if !ok || sender == "" || marker == "" || nonce == "" {
		return false
	}
	fields := make(map[string]string)
	for _, line := range strings.Split(header, "\n") {
		key, value, ok := strings.Cut(line, ":")
		if !ok {
			return false
		}
		key, value = strings.TrimSpace(key), strings.TrimSpace(value)
		if _, duplicate := fields[key]; duplicate {
			return false
		}
		fields[key] = value
	}
	return fields["Message Type"] == "FINAL_ANSWER" && fields["Task name"] == "/root" && fields["Sender"] == sender &&
		strings.Contains(payload, marker) && strings.Contains(payload, nonce)
}

// Exercise the complete shipped oracle through real App Server, Grok projection,
// tool dispatch, a child provider request and parent continuation. Only base_url changes.
func TestShippedChildFixture(t *testing.T) {
	t.Run("delegated_invoice", func(t *testing.T) { testNativeShippedInvoice(t, "delegated_invoice") })
	t.Run("parent_only_invoice_rejected", func(t *testing.T) { testNativeShippedInvoice(t, "parent_only") })
	t.Run("child_without_invoice_rejected", func(t *testing.T) { testNativeShippedInvoice(t, "child_without_invoice") })
	t.Run("inherited_spawn_history", testShippedChildInheritedHistory)
	t.Run("delivered_result", testShippedChildDeliveredResult)
	t.Run("output_schema_scope", testShippedChildOutputSchemaScope)
	t.Run("inherited_result_structured_negative", testShippedChildStructuredNegative)
}

type shippedChildHTTP struct {
	mu                           sync.Mutex
	model, parentID, childID     string
	spawn, wait, nonce, failure  string
	setupMarker, seedEffort      string
	parentSupplied               bool
	requests                     int
	seed                         string
	delegated, waited, continued bool
}
