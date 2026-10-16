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
	return subject
}
