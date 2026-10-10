package live

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/dist"
)

// Exercise the complete shipped oracle through real App Server, Grok projection,
// tool dispatch, child inference and parent continuation. Only base_url changes.
func TestNativeShippedChildFixture(t *testing.T) {
	binary := os.Getenv("GROK_LIVE_NATIVE_BIN")
	if binary == "" {
		t.Skip("activated by the required native runtime job")
	}
	file, err := os.Open(binary)
	if err != nil {
		t.Fatal(err)
	}
	hash := sha256.New()
	_, err = io.Copy(hash, file)
	closeErr := file.Close()
	if err != nil || closeErr != nil {
		t.Fatal("native executable digest unavailable")
	}
	subject := Subject{Binary: binary, SHA256: hex.EncodeToString(hash.Sum(nil)),
		SourceSHA: os.Getenv("GITHUB_SHA"), HarnessSHA: os.Getenv("GITHUB_SHA"),
		Target: "x86_64-unknown-linux-gnu", Environment: "native-fixture"}
	if source := os.Getenv("GROK_LIVE_NATIVE_SOURCE_SHA"); source != "" {
		subject.SourceSHA = source
	}
	if target := os.Getenv("GROK_LIVE_NATIVE_TARGET"); target != "" {
		subject.Target, subject.Environment = target, "package-fixture"
	}
	options, err := shippedOptions(subject, "fixture-key", "")
	if err != nil {
		t.Fatal(err)
	}
	fixture := &shippedChildHTTP{model: options.Model}
	backend := httptest.NewServer(fixture)
	defer backend.Close()
	options.BaseURL = backend.URL + "/v1"
	options.fixtureReady = func(home string) error {
		return substituteShippedEndpoint(home, options.BaseURL)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	got, err := runShippedChildCollaboration(ctx, options)
	fixture.mu.Lock()
	defer fixture.mu.Unlock()
	// No request, identity, UUID or raw protocol content leaves this fixture.
	failure := FailureInfo{}
	if err != nil {
		failure = DescribeFailure(err)
	}
	t.Logf("controlled shipped-child: stage=%s requests=%d seed=%t delegated=%t child_inference=%t parent_replay=%t wire_failure=%s failure=%+v",
		got.Stage, fixture.requests, fixture.seed != nil, fixture.delegated,
		fixture.childID != "", fixture.continued, fixture.failure, failure)
	if err != nil {
		t.Fatalf("native shipped-child oracle: %v", err)
	}
	got.ObservedAt = ""
	want := Evidence{SHA256: subject.SHA256, SourceSHA: subject.SourceSHA, HarnessSHA: subject.HarnessSHA,
		Target: subject.Target, Environment: subject.Environment, Model: options.Model,
		Stage: "child_result_delivered", Processes: 1, Initializations: 1, Threads: 1, Turns: 2,
		SetupTurns: 1, TaskTurns: 1, SetupCompleted: true, ShippedCatalog: true, CatalogModels: 2,
		Bound: true, Completed: true, ReplyBytes: 36, ChildBound: true, ChildCompleted: true, ChildResultDelivered: true}
	requests := 4
	if fixture.waited {
		requests++
	}
	if got != want || fixture.failure != "" || fixture.requests != requests || !fixture.continued {
		t.Fatalf("native shipped-child evidence mismatch: %+v", got)
	}
}

// Test-only, after the ordinary shipped writer. Verify and reread both assets:
// controlled transport must not silently weaken any shipped capability.
func substituteShippedEndpoint(home, endpoint string) error {
	target, err := url.Parse(endpoint)
	if err != nil || target.Scheme != "http" || target.Path != "/v1" ||
		target.User != nil || target.RawQuery != "" || target.Fragment != "" ||
		target.Port() == "" || net.ParseIP(target.Hostname()) == nil || !net.ParseIP(target.Hostname()).IsLoopback() {
		return errors.New("native child: endpoint must be numeric loopback HTTP /v1")
	}
	_, original, err := dist.Defaults()
	if err != nil {
		return err
	}
	path := filepath.Join(home, "config.toml")
	profile, err := os.ReadFile(path)
	if err != nil || string(profile) != dist.Profile() {
		return errors.New("native child: shipped profile changed")
	}
	catalog, err := os.ReadFile(filepath.Join(home, "models.json"))
	if err != nil || !bytes.Equal(catalog, dist.Catalog()) {
		return errors.New("native child: shipped catalog changed")
	}
	oldLine, newLine := fmt.Sprintf("base_url = %q\n", original), fmt.Sprintf("base_url = %q\n", endpoint)
	if strings.Count(string(profile), oldLine) != 1 {
		return errors.New("native child: endpoint line is ambiguous")
	}
	if err := os.WriteFile(path, []byte(strings.Replace(string(profile), oldLine, newLine, 1)), 0600); err != nil {
		return err
	}
	profile, err = os.ReadFile(path)
	if err != nil || strings.Replace(string(profile), newLine, oldLine, 1) != dist.Profile() {
		return errors.New("native child: endpoint substitution changed profile")
	}
	catalog, err = os.ReadFile(filepath.Join(home, "models.json"))
	if err != nil || !bytes.Equal(catalog, dist.Catalog()) {
		return errors.New("native child: endpoint substitution changed catalog")
	}
	return nil
}

const childHTTPTask = "Without running commands or tools, write a fresh UUID v4 yourself and reply with its canonical lowercase text."
const childHTTPSeedReply = "seed accepted"

type childHTTPItem struct {
	Type, ID, Role, Name string
	CallID               string `json:"call_id"`
	Arguments            string
	Output               json.RawMessage
	Content              []struct{ Type, Text string }
}

type shippedChildHTTP struct {
	mu                           sync.Mutex
	model, parentID, childID     string
	spawn, wait, nonce, failure  string
	requests                     int
	seed                         map[string]any
	delegated, waited, continued bool
}

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
		Model         string
		Tools         []json.RawMessage
		Input         []json.RawMessage
		Stream, Store bool
		Reasoning     struct{ Effort string }
	}
	data, err := io.ReadAll(io.LimitReader(request.Body, (1<<20)+1))
	threadID := request.Header.Get("thread-id")
	if err != nil || len(data) > 1<<20 || json.Unmarshal(data, &body) != nil ||
		request.Method != "POST" || request.URL.Path != "/v1/responses" ||
		request.Header.Get("Authorization") != "Bearer fixture-key" || threadID == "" ||
		body.Model != fixture.model || !body.Stream || body.Store || len(body.Input) > 128 || fixture.requests > 5 {
		fail("request_contract")
		return
	}
	// Ultra is the stock local alias for xhigh on the wire.
	effort := "xhigh"
	if fixture.parentID == "" {
		effort = "high"
	}
	if body.Reasoning.Effort != effort {
		fail("reasoning_effort_changed")
		return
	}
	spawn, err := childHTTPDeclaration(body.Tools, "spawn_agent")
	if err != nil {
		fail("unsupported_spawn_declaration")
		return
	}
	wait, err := childHTTPDeclaration(body.Tools, "wait_agent")
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
					var seed map[string]any
					if json.Unmarshal(raw, &seed) != nil || seedAt >= 0 || (fixture.seed != nil && !reflect.DeepEqual(seed, fixture.seed)) {
						fail("seed_replay_changed")
						return
					}
					fixture.seed, seedAt = seed, index
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
	joined := strings.Join(userText, "\n")
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
	newTask := "Message Type: NEW_TASK\nTask name: /root/live_child\nSender: /root\nPayload:\n" + childHTTPTask
	if threadID != fixture.parentID {
		if fixture.childID != "" || !fixture.delegated || !strings.Contains(joined, newTask) || len(calls) != 0 || len(outputs) != 0 {
			fail("child_input_binding")
			return
		}
		fixture.childID = threadID
		var nonce [16]byte
		if _, err := rand.Read(nonce[:]); err != nil {
			fail("nonce_unavailable")
			return
		}
		nonce[6], nonce[8] = nonce[6]&0x0f|0x40, nonce[8]&0x3f|0x80
		fixture.nonce = fmt.Sprintf("%x-%x-%x-%x-%x", nonce[:4], nonce[4:6], nonce[6:8], nonce[8:10], nonce[10:])
		if bytes.Contains(data, []byte(fixture.nonce)) {
			fail("nonce_not_fresh")
			return
		}
		childHTTPReply(writer, "child", fixture.nonce)
		return
	}
	if !fixture.delegated {
		if len(calls) != 0 {
			fail("delegation_input")
			return
		}
		fixture.delegated = true
		childHTTPCall(writer, "child_spawn", fixture.spawn, map[string]any{"task_name": "live_child", "message": childHTTPTask})
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
			Message  string
			TimedOut bool `json:"timed_out"`
		}
		if waitCall.Name != fixture.wait || json.Unmarshal([]byte(waitCall.Arguments), &waitArgs) != nil ||
			!reflect.DeepEqual(waitArgs, map[string]any{"timeout_ms": float64(10000)}) ||
			json.Unmarshal(outputs["child_wait"].Output, &waitText) != nil ||
			json.Unmarshal([]byte(waitText), &result) != nil || result.TimedOut || result.Message != "Wait completed." {
			fail("wait_replay_changed")
			return
		}
	}
	final := "Message Type: FINAL_ANSWER\nTask name: /root\nSender: /root/live_child\nPayload:\n" + fixture.nonce
	if fixture.childID == "" || fixture.nonce == "" || !strings.Contains(joined, final) {
		if fixture.waited {
			fail("parent_completion_missing")
			return
		}
		fixture.waited = true
		childHTTPCall(writer, "child_wait", fixture.wait, map[string]any{"timeout_ms": 10000})
		return
	}
	fixture.continued = true
	childHTTPReply(writer, "parent", fixture.nonce)
}

func childHTTPDeclaration(tools []json.RawMessage, operation string) (string, error) {
	var bound string
	prefix := "Call this function directly to invoke `collaboration." + operation + "`."
	for _, raw := range tools {
		var tool struct {
			Type, Name, Description string
			Parameters              json.RawMessage
		}
		if json.Unmarshal(raw, &tool) != nil {
			return "", errors.New("invalid declaration")
		}
		if !strings.HasPrefix(tool.Description, prefix) {
			continue
		}
		var schema struct {
			Type                 string
			Properties           map[string]struct{ Type string }
			Required             []string
			AdditionalProperties *bool
		}
		if json.Unmarshal(tool.Parameters, &schema) != nil {
			return "", errors.New("unsupported collaboration schema")
		}
		required := []string{"task_name", "message"}
		if operation == "wait_agent" {
			required = nil
		}
		if tool.Type != "function" || bound != "" || tool.Name == "" ||
			schema.Type != "object" || schema.AdditionalProperties == nil || *schema.AdditionalProperties ||
			!reflect.DeepEqual(schema.Required, required) {
			return "", errors.New("unsupported collaboration declaration")
		}
		for name, property := range schema.Properties {
			if operation == "wait_agent" {
				if name != "timeout_ms" || property.Type != "number" {
					return "", errors.New("unsupported wait arguments")
				}
			} else if property.Type != "string" ||
				(name != "task_name" && name != "message" && name != "fork_turns" && name != "agent_type" && name != "model" && name != "reasoning_effort") {
				return "", errors.New("unsupported spawn arguments")
			}
		}
		if operation == "wait_agent" && len(schema.Properties) != 1 ||
			operation == "spawn_agent" && (schema.Properties["task_name"].Type != "string" ||
				schema.Properties["message"].Type != "string" || schema.Properties["fork_turns"].Type != "string") {
			return "", errors.New("missing collaboration arguments")
		}
		bound = tool.Name
	}
	if bound == "" {
		return "", errors.New("direct collaboration declaration absent")
	}
	return bound, nil
}

func childHTTPEvent(writer http.ResponseWriter, event any) {
	encoded, _ := json.Marshal(event)
	_, _ = fmt.Fprintf(writer, "data: %s\n\n", encoded)
}

func childHTTPReply(writer http.ResponseWriter, id, text string) {
	writer.Header().Set("Content-Type", "text/event-stream")
	childHTTPEvent(writer, map[string]any{"type": "response.created", "response": map[string]any{"id": id}})
	item := map[string]any{"type": "message", "id": id + "_message", "role": "assistant", "phase": "final_answer", "content": []any{}}
	childHTTPEvent(writer, map[string]any{"type": "response.output_item.added", "output_index": 0, "item": item})
	childHTTPEvent(writer, map[string]any{"type": "response.output_text.delta", "output_index": 0, "delta": text})
	item["status"], item["content"] = "completed", []any{map[string]any{"type": "output_text", "text": text}}
	childHTTPEvent(writer, map[string]any{"type": "response.output_item.done", "output_index": 0, "item": item})
	childHTTPEvent(writer, map[string]any{"type": "response.completed", "response": map[string]any{"id": id}})
}

func childHTTPCall(writer http.ResponseWriter, id, name string, arguments any) {
	writer.Header().Set("Content-Type", "text/event-stream")
	childHTTPEvent(writer, map[string]any{"type": "response.created", "response": map[string]any{"id": id}})
	args, _ := json.Marshal(arguments)
	item := map[string]any{"type": "function_call", "id": id + "_item", "call_id": id, "name": name, "arguments": ""}
	childHTTPEvent(writer, map[string]any{"type": "response.output_item.added", "output_index": 0, "item": item})
	childHTTPEvent(writer, map[string]any{"type": "response.function_call_arguments.delta", "output_index": 0, "delta": string(args)})
	item["arguments"], item["status"] = string(args), "completed"
	childHTTPEvent(writer, map[string]any{"type": "response.output_item.done", "output_index": 0, "item": item})
	childHTTPEvent(writer, map[string]any{"type": "response.completed", "response": map[string]any{"id": id}})
}

func TestShippedChildDeclarationSchemaBoundary(t *testing.T) {
	const declaration = `{"type":"function","name":"declared_wait_name","description":"Call this function directly to invoke ` + "`collaboration.wait_agent`" + `.","parameters":{"type":"object","properties":{"timeout_ms":{"type":"number"}},"additionalProperties":false}}`
	unrelated := json.RawMessage(`{"type":"function","name":"unrelated","description":"Unrelated shipped capability","parameters":{"type":"object","properties":{"value":{"type":["string","null"]}}}}`)
	for _, tc := range []struct {
		name  string
		tools []json.RawMessage
		want  bool
	}{
		{"unrelated_union", []json.RawMessage{unrelated, json.RawMessage(declaration)}, true},
		{"matched_union", []json.RawMessage{json.RawMessage(strings.Replace(declaration, `"type":"number"`, `"type":["number","null"]`, 1))}, false},
		{"duplicate_target", []json.RawMessage{json.RawMessage(declaration), json.RawMessage(declaration)}, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			name, err := childHTTPDeclaration(tc.tools, "wait_agent")
			if (err == nil) != tc.want || (tc.want && name != "declared_wait_name") {
				t.Fatalf("declaration boundary: admitted=%t name=%q error=%v", err == nil, name, err)
			}
		})
	}
}
