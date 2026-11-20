package live

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func TestDiagnosticProjectionsSkipWorkWithoutSink(t *testing.T) {
	turn := productTurn{ID: "turn", Status: "completed", Items: []protocolv2.ThreadItem{
		protocolv2.NewThreadItemImageGeneration(protocolv2.ThreadItemImageGeneration{
			ID: "image", Status: "completed", Result: strings.Repeat("AAAA", 4096),
		}),
	}}
	stream := childStream{}
	message := frame{Method: "rawResponseItem/completed", Params: json.RawMessage(`{"ThreadID":"parent","TurnID":"turn","Item":{"Type":"message","Role":"assistant","Content":[{"Type":"output_text","Text":"public task result"}]}}`)}
	for name, capture := range map[string]func(){
		"image": func() { recordImageTurn(nil, "generation", turn) },
		"child": func() { stream.recordDiagnostic(message) },
	} {
		t.Run(name, func(t *testing.T) {
			// A disabled projection must return before decoding or building a
			// diagnostic copy of content. This is not a budget for business work.
			if allocations := testing.AllocsPerRun(10, capture); allocations != 0 {
				t.Fatalf("disabled diagnostics processed content: %g allocations", allocations)
			}
		})
	}
}

func TestDiagnosticProjectionRetainsRejectedImageAttempt(t *testing.T) {
	dir := t.TempDir()
	d := NewDiagnostics(dir, "secret-canary")
	turn := productTurn{ID: "turn", Status: "failed", Error: json.RawMessage(`{"message":"public task failure secret-canary","codexErrorInfo":"badRequest"}`), Items: []protocolv2.ThreadItem{
		protocolv2.NewThreadItemImageGeneration(protocolv2.ThreadItemImageGeneration{
			ID: "image", Status: "failed", Result: "not-a-png",
		}),
	}}
	recordProductDiagnosticTurn(d, "inline_snapshot", "thread", turn)
	recordImageTurn(d, "generation", turn)
	if completedProductTurn(turn) == nil {
		t.Fatal("invalid terminal was accepted")
	}
	d.Close()
	data, err := os.ReadFile(filepath.Join(dir, "events.jsonl"))
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{"inline_snapshot", "turn_unsuccessful", "public task failure", "image_attempt", "invalid_png"} {
		if !strings.Contains(string(data), want) {
			t.Errorf("rejected observation lost %q", want)
		}
	}
	if strings.Contains(string(data), "secret-canary") || strings.Contains(string(data), "not-a-png") {
		t.Fatal("diagnostic projection retained a secret or raw image payload")
	}
}

func TestPublicProtocolDiagnosticsRetainErrorsNotPrivatePayload(t *testing.T) {
	dir := t.TempDir()
	d := NewDiagnostics(dir, "secret-canary")
	for _, raw := range []string{
		`{"method":"error","params":{"threadId":"task-thread","error":{"message":"backend overloaded secret-canary","codexErrorInfo":"serverOverloaded"},"willRetry":true}}`,
		`{"method":"item/reasoning/textDelta","params":{"delta":"PRIVATE_REASONING_CANARY"}}`,
		`{"method":"item/agentMessage/delta","params":{"threadId":"task-thread","turnId":"task-turn","delta":"The dog is visible"}}`,
		`{"method":"turn/completed","params":{"turn":{"id":"task-turn","status":"failed","error":{"message":"rate limited"},"items":[]}}}`,
	} {
		var v any
		if err := json.Unmarshal([]byte(raw), &v); err != nil {
			t.Fatal(err)
		}
		protocolDiagnostic(d, "notification", v)
	}
	d.Close()
	data, err := os.ReadFile(filepath.Join(dir, "events.jsonl"))
	if err != nil {
		t.Fatal(err)
	}
	text := string(data)
	for _, wanted := range []string{"backend overloaded", "serverOverloaded", "willRetry", "delta_bytes", "rate limited", "failed"} {
		if !strings.Contains(text, wanted) {
			t.Errorf("missing %s", wanted)
		}
	}
	for _, denied := range []string{"PRIVATE_REASONING_CANARY", "secret-canary", "The dog is visible"} {
		if strings.Contains(text, denied) {
			t.Errorf("leaked %s", denied)
		}
	}
	if len(d.Status().CaptureErrors) != 0 {
		t.Fatalf("unexpected capture failure: %+v", d.Status())
	}
}
