package live

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

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
	for _, wanted := range []string{"backend overloaded", "serverOverloaded", "willRetry", "The dog is visible", "rate limited", "failed"} {
		if !strings.Contains(text, wanted) {
			t.Errorf("missing %s", wanted)
		}
	}
	for _, denied := range []string{"PRIVATE_REASONING_CANARY", "secret-canary"} {
		if strings.Contains(text, denied) {
			t.Errorf("leaked %s", denied)
		}
	}
	if len(d.Status().CaptureErrors) != 0 {
		t.Fatalf("unexpected capture failure: %+v", d.Status())
	}
}
