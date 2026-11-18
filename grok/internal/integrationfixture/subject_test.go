package integrationfixture

import (
	"os"
	"strings"
	"testing"

	"github.com/Harness-X-Harness/codex/grok/internal/testkit"
)

func TestNativeSubjectUsesOwnedSnapshot(t *testing.T) {
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
		subject, cleanup, err := SelectSubject(os.Getenv)
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(cleanup)
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

func TestSubjectBindsArbitraryVerifiedSource(t *testing.T) {
	original, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	source, harness := strings.Repeat("a", 40), strings.Repeat("b", 40)
	env := map[string]string{"GROK_CODEX_BIN": original, "GROK_LIVE_NATIVE_SOURCE_SHA": source, "GITHUB_SHA": harness, "GROK_LIVE_NATIVE_TARGET": "aarch64-apple-darwin"}
	subject, cleanup, err := SelectSubject(func(name string) string { return env[name] })
	if err != nil {
		t.Fatal(err)
	}
	defer cleanup()
	digest, err := testkit.ExecutableSHA256(original)
	if err != nil || subject.LocalUnverified || subject.SourceSHA != source || subject.HarnessSHA != harness || subject.SHA256 != digest || subject.Binary == original || subject.Target != env["GROK_LIVE_NATIVE_TARGET"] || subject.Environment != "package-fixture" {
		t.Fatal("arbitrary source metadata and snapshot digest were not preserved")
	}
}

func TestSubjectRejectsPartialOrMalformedSource(t *testing.T) {
	original, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	for _, tc := range []struct{ name, source, harness string }{
		{"missing_harness", strings.Repeat("a", 40), ""},
		{"short_source", "abc", strings.Repeat("b", 40)},
		{"nonhex_source", strings.Repeat("z", 40), strings.Repeat("b", 40)},
		{"short_harness", strings.Repeat("a", 40), "abc"},
		{"nonhex_harness", strings.Repeat("a", 40), strings.Repeat("z", 40)},
	} {
		t.Run(tc.name, func(t *testing.T) {
			env := map[string]string{"GROK_CODEX_BIN": original, "GROK_LIVE_NATIVE_SOURCE_SHA": tc.source, "GITHUB_SHA": tc.harness}
			_, cleanup, err := SelectSubject(func(name string) string { return env[name] })
			if cleanup != nil {
				cleanup()
			}
			if err == nil {
				t.Fatal("invalid verified source metadata accepted")
			}
		})
	}
}
