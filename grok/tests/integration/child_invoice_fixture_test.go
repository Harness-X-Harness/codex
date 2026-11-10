//go:build integration

package integration_test

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/dist"
	"github.com/Harness-X-Harness/codex/grok/internal/integrationfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

const invoiceCSV = "item,quantity,unit_price,line_total\nnotebook,3,4.50,13.50\npen,2,1.25,2.50\ntotal,,,16.00"
const invoiceChildTask = "Check an order of 3 notebooks at 4.50 each and 2 pens at 1.25 each, with no tax or shipping. Return a CSV invoice with columns item,quantity,unit_price,line_total and a total row with empty quantity and unit_price. Use notebook and pen as item names."

type invoiceHTTP struct {
	mu                                                       sync.Mutex
	mode, model, effort, parent, child, spawn, wait, failure string
	requests                                                 int
	delegated, waited, delivered, continued                  bool
}

func (fixture *invoiceHTTP) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	fixture.mu.Lock()
	defer fixture.mu.Unlock()
	defer r.Body.Close()
	fixture.requests++
	fail := func(reason string) {
		if fixture.failure == "" {
			fixture.failure = reason
		}
		http.Error(w, reason, http.StatusBadRequest)
	}
	var body struct {
		Model  string
		Stream bool
		Store  *bool
		Tools  []json.RawMessage
		Input  []childHTTPItem
		Text   struct {
			Format struct {
				Type   string
				Schema json.RawMessage
			}
		}
		Reasoning struct{ Effort string }
	}
	data, err := io.ReadAll(io.LimitReader(r.Body, (1<<20)+1))
	id := r.Header.Get("thread-id")
	if err != nil || len(data) > 1<<20 || json.Unmarshal(data, &body) != nil || r.Method != "POST" || r.URL.Path != "/v1/responses" || r.Header.Get("Authorization") != "Bearer fixture-key" || id == "" || body.Model != fixture.model || !body.Stream || body.Store == nil || *body.Store || len(body.Input) > 128 || fixture.requests > 5 {
		fail("request_contract")
		return
	}
	if body.Text.Format.Type == "json_schema" || len(body.Text.Format.Schema) != 0 {
		fail("unexpected_output_schema")
		return
	}
	if body.Reasoning.Effort != fixture.effort {
		fail("default_effort_changed")
		return
	}
	spawn, err := integrationfixture.ChildDeclaration(body.Tools, "spawn_agent")
	if err != nil {
		fail("spawn_declaration")
		return
	}
	wait, err := integrationfixture.ChildDeclaration(body.Tools, "wait_agent")
	if err != nil {
		fail("wait_declaration")
		return
	}
	if fixture.spawn == "" {
		fixture.spawn, fixture.wait = spawn, wait
	} else if fixture.spawn != spawn || fixture.wait != wait {
		fail("declaration_identity_changed")
		return
	}
	if fixture.parent == "" {
		fixture.parent = id
	}
	var userText []string
	calls, outputs := make(map[string]childHTTPItem), make(map[string]childHTTPItem)
	for _, item := range body.Input {
		switch item.Type {
		case "message":
			if item.Role == "user" {
				for _, part := range item.Content {
					userText = append(userText, part.Text)
				}
			}
		case "function_call":
			if item.CallID == "" || calls[item.CallID].CallID != "" {
				fail("duplicate_call")
				return
			}
			calls[item.CallID] = item
		case "function_call_output":
			if item.CallID == "" || outputs[item.CallID].CallID != "" || calls[item.CallID].CallID == "" {
				fail("output_binding")
				return
			}
			outputs[item.CallID] = item
		}
	}
	if id != fixture.parent {
		if !fixture.delegated || fixture.child != "" {
			fail("unexpected_child")
			return
		}
		task := false
		for _, text := range userText {
			task = task || strings.Contains(text, invoiceChildTask)
		}
		if !task {
			fail("child_task_absent")
			return
		}
		fixture.child = id
		reply := invoiceCSV
		if fixture.mode == "child_without_invoice" {
			reply = "The invoice is ready."
		}
		childHTTPReply(w, "invoice-child", reply)
		return
	}
	if !fixture.delegated {
		task := false
		for _, text := range userText {
			task = task || strings.Contains(text, "subagent") && strings.Contains(text, "3 notebooks") && strings.Contains(text, "CSV invoice")
		}
		if !task || len(calls) != 0 {
			fail("parent_task_absent")
			return
		}
		if fixture.mode == "parent_only" {
			fixture.continued = true
			childHTTPReply(w, "parent-only", invoiceCSV)
			return
		}
		fixture.delegated = true
		childHTTPCall(w, "invoice_spawn", fixture.spawn, map[string]any{"task_name": "check_invoice", "message": invoiceChildTask})
		return
	}
	var args map[string]any
	var spawnOutput string
	var spawned struct {
		TaskName string `json:"task_name"`
	}
	call := calls["invoice_spawn"]
	if fixture.continued || call.Name != fixture.spawn || json.Unmarshal([]byte(call.Arguments), &args) != nil || !reflect.DeepEqual(args, map[string]any{"task_name": "check_invoice", "message": invoiceChildTask}) || json.Unmarshal(outputs["invoice_spawn"].Output, &spawnOutput) != nil || json.Unmarshal([]byte(spawnOutput), &spawned) != nil || spawned.TaskName != "/root/check_invoice" {
		fail("spawn_replay")
		return
	}
	if fixture.waited {
		args = nil
		call := calls["invoice_wait"]
		var waitOutput string
		var result struct {
			TimedOut *bool `json:"timed_out"`
		}
		if call.Name != fixture.wait || json.Unmarshal([]byte(call.Arguments), &args) != nil || !reflect.DeepEqual(args, map[string]any{"timeout_ms": float64(10000)}) || json.Unmarshal(outputs["invoice_wait"].Output, &waitOutput) != nil || json.Unmarshal([]byte(waitOutput), &result) != nil || result.TimedOut == nil || *result.TimedOut {
			fail("wait_replay")
			return
		}
	}
	reply := invoiceCSV
	if fixture.mode == "child_without_invoice" {
		reply = "The invoice is ready."
	}
	for _, text := range userText {
		fixture.delivered = fixture.delivered || deliveredInvoice(text, spawned.TaskName, reply)
	}
	if fixture.child == "" || !fixture.delivered {
		if fixture.waited {
			fail("child_delivery_absent")
			return
		}
		fixture.waited = true
		childHTTPCall(w, "invoice_wait", fixture.wait, map[string]any{"timeout_ms": 10000})
		return
	}
	fixture.continued = true
	childHTTPReply(w, "invoice-parent", invoiceCSV)
}

func deliveredInvoice(text, sender, reply string) bool {
	header, payload, ok := strings.Cut(text, "\nPayload:\n")
	if !ok {
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
	return fields["Message Type"] == "FINAL_ANSWER" && fields["Task name"] == "/root" && fields["Sender"] == sender && strings.TrimSpace(payload) == reply
}

func testNativeShippedInvoice(t *testing.T, mode string) {
	integrationObservation(t)
	subject := nativeSubject(t)
	model, _, err := dist.Defaults()
	if err != nil {
		t.Fatal(err)
	}
	fixture := &invoiceHTTP{mode: mode, model: model, effort: shippedDefaultEffort(t, model)}
	backend := httptest.NewServer(fixture)
	defer backend.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	ctx = withShippedProfile(t, ctx, backend.URL+"/v1")
	got, err := live.ShippedChildCollaboration(ctx, subject, "fixture-key")
	fixture.mu.Lock()
	defer fixture.mu.Unlock()
	failure := live.DescribeFailure(err)
	t.Logf("controlled invoice: stage=%s requests=%d delegated=%t child_request=%t delivered=%t continued=%t wire_failure=%s failure=%+v", got.Stage, fixture.requests, fixture.delegated, fixture.child != "", fixture.delivered, fixture.continued, fixture.failure, failure)
	if fixture.failure != "" || !fixture.continued || got.CleanupPending || got.Turns != 1 || got.SetupTurns != 0 || got.SetupCompleted || got.TaskTurns != 1 {
		t.Fatalf("native invoice path incomplete: %+v error=%v", got, err)
	}
	if mode == "parent_only" {
		if err == nil || failure.Code != "child_binding_not_observed" || got.Stage != "product_turn_proven" || fixture.requests != 1 || got.ChildBound || got.ChildCompleted || got.ChildResultDelivered {
			t.Fatalf("parent-only invoice falsely accepted: %+v %v", got, err)
		}
		return
	}
	requests := 3
	if fixture.waited {
		requests++
	}
	if fixture.requests != requests || fixture.child == "" || !fixture.delivered || !got.ChildBound || !got.ChildCompleted {
		t.Fatalf("actual child participation not reached: %+v %v", got, err)
	}
	if mode == "child_without_invoice" {
		if err == nil || failure.Code != "child_invoice_not_observed" || got.ChildResultDelivered {
			t.Fatalf("ready-only child falsely accepted: %+v %v", got, err)
		}
		return
	}
	if err != nil || !got.Completed || !got.ChildResultDelivered || got.Stage != "child_result_delivered" || got.LocalUnverified != subject.LocalUnverified || got.SHA256 != subject.SHA256 || got.SourceSHA != subject.SourceSHA || got.HarnessSHA != subject.HarnessSHA || got.Target != subject.Target || got.Environment != subject.Environment || got.Model != model {
		t.Fatalf("native invoice proof: %+v %v", got, err)
	}
}

// These transcript mutations exercise the fixture, not a native child runtime.
// The three mandatory native subtests above own actual composition.
func TestChildInvoiceHTTPFixtureTranscript(t *testing.T) {
	integrationObservation(t)
	message := func(text string) map[string]any {
		return map[string]any{"type": "message", "role": "user", "content": []any{map[string]any{"type": "input_text", "text": text}}}
	}
	for _, mode := range []string{"delegated_invoice", "child_without_invoice", "parent_only", "wrong_sender", "missing_child_task"} {
		t.Run(mode, func(t *testing.T) {
			fixture := &invoiceHTTP{mode: mode, model: "fixture-model", effort: "high"}
			request := func(id string, input []map[string]any) *httptest.ResponseRecorder {
				t.Helper()
				tools := []any{map[string]any{"type": "function", "name": "fixture_spawn", "description": "Call this function directly to invoke `collaboration.spawn_agent`."}, map[string]any{"type": "function", "name": "fixture_wait", "description": "Call this function directly to invoke `collaboration.wait_agent`."}}
				data, err := json.Marshal(map[string]any{"model": fixture.model, "stream": true, "store": false, "reasoning": map[string]any{"effort": "high"}, "tools": tools, "input": input})
				if err != nil {
					t.Fatal(err)
				}
				req := httptest.NewRequest(http.MethodPost, "/v1/responses", strings.NewReader(string(data)))
				req.Header.Set("Authorization", "Bearer fixture-key")
				req.Header.Set("thread-id", id)
				out := httptest.NewRecorder()
				fixture.ServeHTTP(out, req)
				return out
			}
			task := message("Please ask a subagent to check 3 notebooks and prepare a CSV invoice.")
			if response := request("parent", []map[string]any{task}); response.Code != http.StatusOK {
				t.Fatalf("first parent request: %s", response.Body.String())
			}
			if mode == "parent_only" {
				if !fixture.continued || fixture.delegated || fixture.requests != 1 {
					t.Fatal("parent-only fixture changed")
				}
				return
			}
			childTask := invoiceChildTask
			if mode == "missing_child_task" {
				childTask = "wrong task"
			}
			child := request("child", []map[string]any{message(childTask)})
			if mode == "missing_child_task" {
				if child.Code != http.StatusBadRequest || fixture.child != "" || fixture.failure != "child_task_absent" {
					t.Fatal("missing child task accepted")
				}
				return
			}
			if child.Code != http.StatusOK {
				t.Fatalf("child request: %s", child.Body.String())
			}
			args, _ := json.Marshal(map[string]any{"task_name": "check_invoice", "message": invoiceChildTask})
			sender := "/root/check_invoice"
			if mode == "wrong_sender" {
				sender = "/root/other"
			}
			reply := invoiceCSV
			if mode == "child_without_invoice" {
				reply = "The invoice is ready."
			}
			delivered := "Message Type: FINAL_ANSWER\nTask name: /root\nSender: " + sender + "\nPayload:\n" + reply
			input := []map[string]any{task, {"type": "function_call", "call_id": "invoice_spawn", "name": "fixture_spawn", "arguments": string(args)}, {"type": "function_call_output", "call_id": "invoice_spawn", "output": `{"task_name":"/root/check_invoice"}`}, message(delivered)}
			response := request("parent", input)
			if response.Code != http.StatusOK {
				t.Fatalf("parent continuation: %s", response.Body.String())
			}
			if mode == "wrong_sender" {
				if fixture.delivered || fixture.continued || !fixture.waited {
					t.Fatal("wrong sender credited")
				}
				return
			}
			if !fixture.delivered || !fixture.continued || fixture.requests != 3 || fixture.failure != "" {
				t.Fatalf("fixture did not prove path: %+v", fixture)
			}
		})
	}
}
