package live

import (
	"os"
	"runtime"
	"testing"

	"github.com/Harness-X-Harness/codex/grok/internal/testkit"
)

func nativeInternalSubject(t *testing.T) Subject {
	t.Helper()
	binary, err := testkit.SelectBinary(os.Getenv)
	if err != nil {
		t.Fatal(err)
	}
	digest, err := testkit.ExecutableSHA256(binary)
	if err != nil {
		t.Fatal(err)
	}
	subject := Subject{Binary: binary, SHA256: digest, Target: runtime.GOOS + "-" + runtime.GOARCH, Environment: "local-integration", LocalUnverified: true}
	source, harness := os.Getenv("GROK_LIVE_NATIVE_SOURCE_SHA"), os.Getenv("GITHUB_SHA")
	if source == "" {
		source = harness
	}
	if source != "" || harness != "" {
		if source == "" || harness == "" {
			t.Fatal("verified integration requires both source and harness revisions")
		}
		subject.SourceSHA, subject.HarnessSHA, subject.LocalUnverified = source, harness, false
		subject.Target, subject.Environment = "x86_64-unknown-linux-gnu", "native-fixture"
		if source == "e9268d5211064fb77f76a9fe7f0a99348488c9d2" && digest != "26ee0bce9bbcb4817d87b99a31a75367e5dfe142d5e9e1f2cc93468610faf1ab" {
			t.Fatal("frozen product executable digest does not match the accepted package")
		}
	}
	if target := os.Getenv("GROK_LIVE_NATIVE_TARGET"); target != "" {
		subject.Target = target
		if !subject.LocalUnverified {
			subject.Environment = "package-fixture"
		}
	}
	// Preserve the package/helper layout while binding the consumed bytes to
	// an owned copy rather than the developer's mutable build output path.
	snapshot, cleanup, err := testkit.SnapshotExecutable(binary, digest)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		// Positive owners require a joined scenario; intentional-negative owners
		// also assert !CleanupPending. A failing test conservatively retains.
		if t.Failed() {
			t.Log("execution snapshot retained after failed observation; cleanup requires confirming child termination")
			return
		}
		cleanup()
	})
	subject.Binary = snapshot
	return subject
}

func TestNativeInternalSubjectUsesOwnedSnapshot(t *testing.T) {
	original, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	var snapshot string
	t.Run("selection", func(t *testing.T) {
		for _, name := range []string{"GROK_LIVE_NATIVE_BIN", "GROK_LIVE_NATIVE_SOURCE_SHA", "GROK_LIVE_NATIVE_TARGET", "GITHUB_SHA"} {
			t.Setenv(name, "")
		}
		t.Setenv("GROK_CODEX_BIN", original)
		subject := nativeInternalSubject(t)
		snapshot = subject.Binary
		digest, err := testkit.ExecutableSHA256(original)
		if err != nil || snapshot == original || subject.SHA256 != digest || !subject.LocalUnverified || subject.SourceSHA != "" || subject.HarnessSHA != "" {
			t.Fatal("local selection did not preserve identity through an owned snapshot")
		}
	})
	if _, err := os.Stat(snapshot); !os.IsNotExist(err) {
		t.Fatal("native test snapshot was not removed by its owner")
	}
}
