package live

import (
	"bytes"
	"encoding/json"
	"image"
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

func awaitDiagnosticSignal(t *testing.T, signal <-chan struct{}) {
	t.Helper()
	select {
	case <-signal:
	case <-time.After(2 * time.Second):
		t.Fatal("diagnostic cut point was not reached")
	}
}

func readDiagnosticManifest(t *testing.T, dir string) DiagnosticsStatus {
	t.Helper()
	var status DiagnosticsStatus
	data, err := os.ReadFile(filepath.Join(dir, "manifest.json"))
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(data, &status); err != nil {
		t.Fatal(err)
	}
	return status
}

func TestDiagnosticsAsyncImmutableSnapshotsAndOrder(t *testing.T) {
	dir := t.TempDir()
	entered, release := make(chan struct{}), make(chan struct{})
	var once sync.Once
	d := newDiagnostics(dir, diagnosticWriterHooks{beforeWrite: func(string) {
		once.Do(func() { close(entered); <-release })
	}}, "SECRET_CANARY")
	t.Cleanup(func() {
		select {
		case <-release:
		default:
			close(release)
		}
		d.Close()
	})
	fields := map[string]any{"nested": map[string]any{"text": "before SECRET_CANARY"}, "values": []any{"original"}}
	d.Record("first", fields)
	awaitDiagnosticSignal(t, entered)
	fields["nested"].(map[string]any)["text"] = "MUTATED_MAP"
	fields["values"].([]any)[0] = "MUTATED_SLICE"
	text := []byte("original task text")
	if d.Artifact("copy.txt", text) != "copy.txt" {
		t.Fatal("text was not admitted")
	}
	copy(text, "MUTATED_TEXT")
	var pixels bytes.Buffer
	if err := png.Encode(&pixels, image.NewRGBA(image.Rect(0, 0, 1, 1))); err != nil {
		t.Fatal(err)
	}
	originalPNG := bytes.Clone(pixels.Bytes())
	if d.Artifact("copy.png", pixels.Bytes()) != "copy.png" {
		t.Fatal("PNG was not admitted")
	}
	copy(pixels.Bytes(), []byte("MUTATED_PNG"))
	d.Record("second", map[string]any{"order": 2})
	d.Record("third", map[string]any{"order": 3})
	if got := d.Status(); got.Events != 0 || got.Artifacts != 0 || got.PendingWrites != 5 {
		t.Fatalf("admission reported as persistence: %+v", got)
	}
	close(release)
	d.Close()
	events, _ := os.ReadFile(filepath.Join(dir, "events.jsonl"))
	lines := bytes.Split(bytes.TrimSpace(events), []byte{'\n'})
	if len(lines) != 3 {
		t.Fatalf("events: %q", events)
	}
	for i, kind := range []string{"first", "second", "third"} {
		var event struct {
			Kind string `json:"kind"`
		}
		if err := json.Unmarshal(lines[i], &event); err != nil || event.Kind != kind {
			t.Fatalf("event order %d: %s (%v)", i, lines[i], err)
		}
	}
	all := diagnosticFiles(t, dir)
	if strings.Contains(all, "MUTATED") || strings.Contains(all, "SECRET_CANARY") || !strings.Contains(all, "original task text") {
		t.Fatal("queued evidence was mutable or unsanitized")
	}
	actualPNG, _ := os.ReadFile(filepath.Join(dir, "copy.png"))
	if !bytes.Equal(actualPNG, originalPNG) {
		t.Fatal("queued PNG changed after caller mutation")
	}
}

func TestDiagnosticsAsyncQueueIsBoundedWithoutWaitingForIO(t *testing.T) {
	entered, release := make(chan struct{}), make(chan struct{})
	var once sync.Once
	d := newDiagnostics(t.TempDir(), diagnosticWriterHooks{
		beforeWrite:  func(string) { once.Do(func() { close(entered); <-release }) },
		flushTimeout: 20 * time.Millisecond,
	})
	t.Cleanup(func() {
		select {
		case <-release:
		default:
			close(release)
		}
		d.Close()
	})
	d.Record("in_flight", nil)
	awaitDiagnosticSignal(t, entered)
	for i := 0; i < diagnosticsQueueSize; i++ {
		d.Record("queued", map[string]any{"i": i})
	}
	returned := make(chan struct{})
	go func() {
		d.Record("overflow", nil)
		d.Artifact("overflow.txt", []byte("overflow"))
		close(returned)
	}()
	awaitDiagnosticSignal(t, returned)
	if got := d.Status(); got.PendingWrites != diagnosticsQueueSize+1 || got.DroppedEvents != 1 || got.DroppedArtifacts != 1 || !got.Truncated || got.Events != 0 {
		t.Fatalf("queue accounting: %+v", got)
	}
	d.Close()
	if got := d.Status(); !got.Closed || !got.FlushTimedOut || got.PendingWrites != diagnosticsQueueSize+1 {
		t.Fatalf("blocked drain hidden: %+v", got)
	}
	select {
	case <-d.done:
		t.Fatal("closed capture was confused with joined writer")
	default:
	}
	d.Close() // Idempotent and bounded even while the worker remains blocked.
	close(release)
	awaitDiagnosticSignal(t, d.done)
	if got := d.Status(); got.PendingWrites != 0 || !got.FlushTimedOut || got.Events != diagnosticsQueueSize+1 {
		t.Fatalf("late drain: %+v", got)
	}
}

func TestDiagnosticsAsyncByteAndEventReservationsIncludeInFlight(t *testing.T) {
	for _, kind := range []string{"bytes", "events"} {
		t.Run(kind, func(t *testing.T) {
			entered, release := make(chan struct{}), make(chan struct{})
			var once sync.Once
			d := newDiagnostics(t.TempDir(), diagnosticWriterHooks{beforeWrite: func(string) {
				once.Do(func() { close(entered); <-release })
			}})
			t.Cleanup(func() {
				select {
				case <-release:
				default:
					close(release)
				}
				d.Close()
			})
			if kind == "bytes" {
				d.maxBytes = 16
				d.Artifact("first.txt", []byte("12345678"))
				awaitDiagnosticSignal(t, entered)
				d.Artifact("second.txt", []byte("abcdefgh"))
				if d.Artifact("third.txt", []byte("x")) != "" {
					t.Fatal("in-flight artifact bytes escaped total budget")
				}
				if got := d.Status(); got.Bytes != 0 || got.Artifacts != 0 || got.PendingWrites != 2 || got.DroppedArtifacts != 1 {
					t.Fatalf("byte reservation: %+v", got)
				}
			} else {
				d.maxEvents = 1
				d.Record("first", nil)
				awaitDiagnosticSignal(t, entered)
				d.Record("second", nil)
				if got := d.Status(); got.Events != 0 || got.PendingWrites != 1 || got.DroppedEvents != 1 {
					t.Fatalf("event reservation: %+v", got)
				}
			}
			close(release)
			waitDiagnostic(t, func() bool { return d.Status().PendingWrites == 0 })
			if kind == "bytes" && d.Artifact("late.txt", []byte("x")) != "" {
				t.Fatal("written bytes were released from the run budget")
			}
			d.Close()
		})
	}
}

func TestDiagnosticsAsyncConcurrentRecordAndClose(t *testing.T) {
	d := NewDiagnostics(t.TempDir())
	var workers sync.WaitGroup
	for i := 0; i < 64; i++ {
		workers.Add(1)
		go func(i int) {
			defer workers.Done()
			d.Record("concurrent", map[string]any{"i": i})
			if i%8 == 0 {
				d.Close()
			}
		}(i)
	}
	workers.Wait()
	d.Close()
	before := d.Status()
	d.Record("late", nil)
	if d.Artifact("late.txt", []byte("late")) != "" {
		t.Fatal("artifact admitted after close")
	}
	after := d.Status()
	if !after.Closed || after.PendingWrites != 0 || after.Events != before.Events || after.DroppedEvents != before.DroppedEvents+1 || after.DroppedArtifacts != before.DroppedArtifacts+1 {
		t.Fatalf("close admission: before=%+v after=%+v", before, after)
	}
}

func TestDiagnosticsAsyncTimeoutDuringFinalManifestIsRetained(t *testing.T) {
	dir := t.TempDir()
	entered, release, closeReturned := make(chan struct{}), make(chan struct{}), make(chan struct{})
	var once sync.Once
	d := newDiagnostics(dir, diagnosticWriterHooks{
		beforeManifest: func(status DiagnosticsStatus) {
			if status.Closed {
				once.Do(func() { close(entered); <-release })
			}
		},
		flushTimeout: 20 * time.Millisecond,
	})
	t.Cleanup(func() {
		select {
		case <-release:
		default:
			close(release)
		}
		d.Close()
	})
	go func() { d.Close(); close(closeReturned) }()
	awaitDiagnosticSignal(t, entered)
	awaitDiagnosticSignal(t, closeReturned)
	if !d.Status().FlushTimedOut {
		t.Fatal("blocked final manifest was not reported")
	}
	close(release)
	awaitDiagnosticSignal(t, d.done)
	got := readDiagnosticManifest(t, dir)
	if !got.Closed || !got.FlushTimedOut || got.PendingWrites != 0 {
		t.Fatalf("late manifest lost timeout: %+v", got)
	}
}

func TestDiagnosticsAsyncFailedArtifactIsNotCountedAsWritten(t *testing.T) {
	dir := t.TempDir()
	d := NewDiagnostics(dir)
	if err := os.WriteFile(filepath.Join(dir, "existing.txt"), []byte("preserve"), 0600); err != nil {
		t.Fatal(err)
	}
	if d.Artifact("existing.txt", []byte("replacement")) != "existing.txt" {
		t.Fatal("valid queued reference was not returned")
	}
	d.Close()
	data, _ := os.ReadFile(filepath.Join(dir, "existing.txt"))
	if string(data) != "preserve" || d.Status().Artifacts != 0 || d.Status().PendingWrites != 0 || len(d.Status().CaptureErrors) == 0 {
		t.Fatalf("failed persistence reported as success: %+v", d.Status())
	}
}
