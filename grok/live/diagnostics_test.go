package live

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

func waitDiagnostic(t *testing.T, ready func() bool) {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for !ready() {
		if time.Now().After(deadline) {
			t.Fatal("diagnostic writer did not reach expected state")
		}
		time.Sleep(time.Millisecond)
	}
}

func diagnosticFiles(t *testing.T, dir string) string {
	t.Helper()
	var all strings.Builder
	files, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	for _, file := range files {
		if file.IsDir() {
			continue
		}
		data, err := os.ReadFile(filepath.Join(dir, file.Name()))
		if err != nil {
			t.Fatal(err)
		}
		all.Write(data)
	}
	return all.String()
}

func TestDiagnosticsIncrementalSanitizedPublicEvidence(t *testing.T) {
	dir := t.TempDir()
	d := NewDiagnostics(dir, "KNOWN_CANARY")
	d.Record("request", map[string]any{
		"prompt": "Draw a dog", "arguments": json.RawMessage(`{"path":"fixture.txt","password":"PASSWORD_CANARY","nested":{"api_key":"KEY_CANARY","content":"requested edit"}}`),
		"credentials": "CREDENTIAL_CANARY", "environment_variables": map[string]any{"API": "ENV_CANARY"},
		"raw_response": "RAW_CANARY", "encrypted_content": "CIPHER_CANARY",
		"output":                 "KNOWN_CANARY Authorization: Bearer AUTH_CANARY https://user:URL_CANARY@example.com/a?token=QUERY_CANARY&safe=yes",
		"json_arguments":         `{"password":"QUOTED CANARY","api_key":"OTHER_KEY_CANARY","text":"okay"}`,
		"command":                "helper --api-key FLAG_CANARY",
		"cookie":                 "private",
		"headers_text":           "Cookie: a=COOKIE_A_CANARY; b=COOKIE_B_CANARY\nAuthorization: Basic BASIC_CANARY",
		"signed_url":             "https://example.com/a?sig=SIGNED_CANARY&safe=yes",
		"defaultReasoningEffort": "high", "supportedReasoningEfforts": []string{"low", "high"}, "token": "business-token",
	})
	waitDiagnostic(t, func() bool {
		data, err := os.ReadFile(filepath.Join(dir, "events.jsonl"))
		return err == nil && bytes.Contains(data, []byte("Draw a dog"))
	})
	d.Item("turn", json.RawMessage(`{"type":"reasoning","id":"r","summary":"REASONING_CANARY","encryptedContent":"OPAQUE_CANARY"}`))
	d.Item("turn", json.RawMessage(`{"type":"newPrivateItem","id":"x","payload":"UNKNOWN_CANARY"}`))
	d.Item("turn", json.RawMessage(`{"type":"commandExecution","id":"c","aggregatedOutput":"completed file edit","exitCode":0}`))
	d.Item("turn", json.RawMessage(`{"type":"imageGeneration","id":"i","result":"IMAGE_PAYLOAD_CANARY","status":"completed"}`))
	d.Close()
	all := diagnosticFiles(t, dir)
	for _, secret := range []string{"KNOWN_CANARY", "PASSWORD_CANARY", "KEY_CANARY", "CREDENTIAL_CANARY", "ENV_CANARY", "RAW_CANARY", "CIPHER_CANARY", "AUTH_CANARY", "URL_CANARY", "QUERY_CANARY", "QUOTED CANARY", "OTHER_KEY_CANARY", "COOKIE_A_CANARY", "COOKIE_B_CANARY", "BASIC_CANARY", "SIGNED_CANARY", "FLAG_CANARY", "REASONING_CANARY", "OPAQUE_CANARY", "UNKNOWN_CANARY", "IMAGE_PAYLOAD_CANARY"} {
		if strings.Contains(all, secret) {
			t.Fatalf("secret exported: %s", secret)
		}
	}
	for _, fact := range []string{"Draw a dog", "requested edit", "completed file edit", "payloadBytes", "metadata_only", "defaultReasoningEffort", "supportedReasoningEfforts", "business-token"} {
		if !strings.Contains(all, fact) {
			t.Fatalf("public evidence lost: %s", fact)
		}
	}
	if got := d.Status(); got.Events != 5 || !got.Closed || len(got.CaptureErrors) != 1 || got.CaptureErrors[0] != "image_payload_invalid" {
		t.Fatalf("status: %+v", got)
	}
}

func TestDiagnosticsPNGAndTextArtifacts(t *testing.T) {
	dir := t.TempDir()
	d := NewDiagnostics(dir, "TEXT_CANARY")
	canvas := image.NewRGBA(image.Rect(0, 0, 2, 2))
	canvas.Set(0, 0, color.RGBA{R: 255, A: 255})
	var buffer bytes.Buffer
	if err := png.Encode(&buffer, canvas); err != nil {
		t.Fatal(err)
	}
	if got := d.Artifact("generated-1.png", buffer.Bytes()); got != "generated-1.png" {
		t.Fatalf("PNG missing: %s %+v", got, d.Status())
	}
	if got := d.Artifact("edit-before.txt", []byte("before TEXT_CANARY password=OTHER_CANARY")); got == "" {
		t.Fatal("text missing")
	}
	if got := d.Artifact("edit.diff", []byte("-old\n+new")); got == "" {
		t.Fatal("diff missing")
	}
	if got := d.Artifact("empty-before.txt", nil); got != "empty-before.txt" {
		t.Fatal("valid empty text artifact rejected")
	}
	d.Close()
	data, _ := os.ReadFile(filepath.Join(dir, "generated-1.png"))
	if !bytes.Equal(data, buffer.Bytes()) {
		t.Fatal("pixels re-encoded or changed")
	}
	text, _ := os.ReadFile(filepath.Join(dir, "edit-before.txt"))
	if bytes.Contains(text, []byte("CANARY")) {
		t.Fatal("text secret exported")
	}
	if got := d.Status(); got.Artifacts != 4 {
		t.Fatalf("artifacts: %+v", got)
	}
}

func TestDiagnosticsLimitsKeepExplicitManifest(t *testing.T) {
	dir := t.TempDir()
	d := NewDiagnostics(dir)
	d.maxEvents = 1
	d.Record("first", map[string]any{"text": "positive evidence"})
	d.Record("second", nil)
	d.Close()
	var status DiagnosticsStatus
	data, _ := os.ReadFile(filepath.Join(dir, "manifest.json"))
	if err := json.Unmarshal(data, &status); err != nil {
		t.Fatal(err)
	}
	if !status.Truncated || status.Events != 1 || len(status.CaptureErrors) == 0 {
		t.Fatalf("silent loss: %+v", status)
	}
	for _, mode := range []string{"event_bytes", "total_bytes", "artifact_bytes"} {
		t.Run(mode, func(t *testing.T) {
			d := NewDiagnostics(t.TempDir())
			switch mode {
			case "event_bytes":
				d.Record("large", map[string]any{"text": strings.Repeat("x", diagnosticsMaxEventBytes+1)})
			case "total_bytes":
				d.maxBytes = 1
				d.Record("small", nil)
			case "artifact_bytes":
				d.Artifact("large.txt", make([]byte, diagnosticsMaxArtifactBytes+1))
			}
			d.Close()
			if !d.Status().Truncated {
				t.Fatal("limit not reported")
			}
		})
	}
}

func TestDiagnosticsFailuresAndSymlinksDoNotReplaceEvidence(t *testing.T) {
	dir := t.TempDir()
	sentinel := filepath.Join(dir, "events.jsonl")
	os.WriteFile(sentinel, []byte("preserve"), 0600)
	d := NewDiagnostics(dir)
	d.Record("not_written", nil)
	d.Close()
	data, _ := os.ReadFile(sentinel)
	if string(data) != "preserve" {
		t.Fatal("existing evidence overwritten")
	}
	if len(d.Status().CaptureErrors) == 0 {
		t.Fatal("capture failure hidden")
	}
	dir = t.TempDir()
	d = NewDiagnostics(dir)
	outside := filepath.Join(t.TempDir(), "outside.txt")
	os.WriteFile(outside, []byte("preserve"), 0600)
	if err := os.Symlink(outside, filepath.Join(dir, "linked.txt")); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"../outside.txt", "arbitrary.json"} {
		if d.Artifact(name, []byte("overwrite")) != "" {
			t.Fatal("unsafe artifact accepted")
		}
	}
	// A syntactically safe name can be admitted before the writer discovers
	// a preexisting symlink. That capture failure must not follow or replace it.
	d.Artifact("linked.txt", []byte("overwrite"))
	waitDiagnostic(t, func() bool { return d.Status().PendingWrites == 0 })
	data, _ = os.ReadFile(outside)
	if string(data) != "preserve" {
		t.Fatal("symlink followed")
	}
	d.events.Close()
	d.Record("closed_stream", nil)
	d.Close()
	if len(d.Status().CaptureErrors) == 0 {
		t.Fatal("write failure hidden")
	}
	path := filepath.Join(t.TempDir(), "not-directory")
	os.WriteFile(path, []byte("file"), 0600)
	d = NewDiagnostics(path)
	d.Record("noop", nil)
	d.Close()
	if len(d.Status().CaptureErrors) == 0 {
		t.Fatal("directory failure hidden")
	}
}

func TestDiagnosticsNilAndConcurrentCapture(t *testing.T) {
	var none *Diagnostics
	none.Record("noop", nil)
	none.Item("noop", nil)
	none.Close()
	if none.Artifact("none.txt", []byte("none")) != "" {
		t.Fatal("nil sink created artifact")
	}
	_ = none.Status()
	d := NewDiagnostics(t.TempDir())
	var wait sync.WaitGroup
	for i := 0; i < 20; i++ {
		wait.Add(1)
		go func() { defer wait.Done(); d.Record("parallel", map[string]any{"result": "ok"}) }()
	}
	wait.Wait()
	d.Close()
	if d.Status().Events != 20 {
		t.Fatal("concurrent evidence lost")
	}
}

func TestDiagnosticsManifestIsIncrementalAndSurvivesMissingClose(t *testing.T) {
	dir := t.TempDir()
	d := NewDiagnostics(dir)
	read := func() DiagnosticsStatus {
		t.Helper()
		var got DiagnosticsStatus
		data, err := os.ReadFile(filepath.Join(dir, "manifest.json"))
		if err != nil {
			t.Fatal(err)
		}
		if json.Unmarshal(data, &got) != nil {
			t.Fatal("invalid manifest")
		}
		return got
	}
	if got := read(); got.Closed || !got.Enabled || got.Events != 0 {
		t.Fatalf("initial manifest: %+v", got)
	}
	d.Record("partial", map[string]any{"text": "visible partial answer"})
	waitDiagnostic(t, func() bool { return read().Events == 1 })
	if got := read(); got.Events != 1 || got.Closed {
		t.Fatalf("partial manifest: %+v", got)
	}
	d.maxEvents = 1
	d.Record("overflow", nil)
	waitDiagnostic(t, func() bool { return read().Truncated })
	if got := read(); !got.Truncated || got.Closed {
		t.Fatalf("limit manifest: %+v", got)
	}
	d.Close()
}

func TestDiagnosticsPublicImagesAreSavedBeforeFailedTurnAndDeduplicated(t *testing.T) {
	dir := t.TempDir()
	d := NewDiagnostics(dir)
	var pixels bytes.Buffer
	png.Encode(&pixels, image.NewRGBA(image.Rect(0, 0, 1, 1)))
	item, _ := json.Marshal(map[string]any{"type": "imageGeneration", "id": "recovery-image", "status": "completed", "result": base64.StdEncoding.EncodeToString(pixels.Bytes())})
	d.Item("item/completed", item)
	d.Item("turn/completed", item)
	d.Record("turn_failed", map[string]any{"status": "failed"})
	d.Item("turn", json.RawMessage(`{"type":"subAgentActivity","id":"child","status":"completed","agentsStates":{"child1":{"status":"completed"}}}`))
	d.Close()
	if got := d.Status(); got.Artifacts != 1 || len(got.CaptureErrors) != 0 {
		t.Fatalf("image capture: %+v", got)
	}
	files, _ := filepath.Glob(filepath.Join(dir, "image-*.png"))
	if len(files) != 1 {
		t.Fatal("public image missing")
	}
	data, _ := os.ReadFile(files[0])
	if !bytes.Equal(data, pixels.Bytes()) {
		t.Fatal("actual image changed")
	}
	events, _ := os.ReadFile(filepath.Join(dir, "events.jsonl"))
	if !bytes.Contains(events, []byte("artifact")) || !bytes.Contains(events, []byte("agentsStates")) {
		t.Fatal("public evidence missing")
	}
}
