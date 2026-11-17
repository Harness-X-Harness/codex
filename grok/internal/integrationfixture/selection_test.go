package integrationfixture

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"
)

// Exercise the native package's own entry guard without launching an App Server
// or contacting a provider. A synthetic executable is only snapshotted in the
// empty-descendant control; it is never credited as a product observation.
func TestIntegrationSelectionFailsClosed(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), time.Minute)
	defer cancel()
	suffix := ""
	if runtime.GOOS == "windows" {
		suffix = ".exe"
	}
	binary := filepath.Join(t.TempDir(), "integration"+suffix)
	build := exec.CommandContext(ctx, "go", "test", "-c", "-tags=integration", "-o", binary, "../../tests/integration")
	if output, err := build.CombinedOutput(); err != nil {
		t.Fatalf("cannot compile Integration guard: %v\n%s", err, output)
	}
	original, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	env := os.Environ()
	for _, key := range []string{"GROK_CODEX_BIN", "GROK_LIVE_NATIVE_BIN", "GROK_LIVE_NATIVE_SOURCE_SHA", "GROK_LIVE_NATIVE_TARGET", "GITHUB_SHA"} {
		prefix := key + "="
		filtered := env[:0]
		for _, entry := range env {
			if !strings.HasPrefix(entry, prefix) {
				filtered = append(filtered, entry)
			}
		}
		env = filtered
	}
	env = append(env, "GROK_CODEX_BIN="+original)
	for _, tc := range []struct {
		name string
		args []string
		want string
	}{
		{"absent_owner", []string{"-test.run=^Absent$"}, "no observations selected"},
		{"empty_group", []string{"-test.run=^TestStructuredEditFixture$/^absent$"}, "no observations selected"},
		{"empty_subject_owner", []string{"-test.run=^TestHostedSearchFixture$/^absent$"}, "no observations selected"},
		{"skipped", []string{"-test.skip=Test"}, "skipped selection forbidden"},
		{"zero_count", []string{"-test.count=0"}, "repeated or zero execution forbidden"},
		{"repeated", []string{"-test.count=2"}, "repeated or zero execution forbidden"},
		{"cpu_repetitions", []string{"-test.cpu=1,2"}, "repeated or zero execution forbidden"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			cmd := exec.CommandContext(ctx, binary, tc.args...)
			cmd.Env = env
			output, err := cmd.CombinedOutput()
			if err == nil || !strings.Contains(string(output), tc.want) {
				t.Fatalf("selection did not fail through its bounded guard: error=%v output=%s", err, output)
			}
		})
	}
	list := exec.CommandContext(ctx, binary, "-test.list=^Test")
	list.Env = env
	output, err := list.CombinedOutput()
	if err != nil || !strings.Contains(string(output), "TestHostedSearchFixture") {
		t.Fatal("credential-free owner discovery failed")
	}
}
