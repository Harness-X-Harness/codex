package live_test

import (
	"context"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/live"
)

// Inspect persisted files, not an in-memory writer mock: diagnostics must survive
// fixture cleanup on both accepted and rejected observations.
func retainedRouteDiagnostics(t *testing.T, dir string) string {
	t.Helper()
	var result strings.Builder
	err := filepath.WalkDir(dir, func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if entry.IsDir() {
			return nil
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		result.Write(data)
		result.WriteByte('\n')
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	return result.String()
}

func TestTextRoutePersistedDiagnostics(t *testing.T) {
	for _, tc := range []struct {
		name, mode         string
		reasoning, failure bool
		wanted             []string
	}{
		{"basic-success", "turn", false, false, []string{"turn_input", "short confirmation", "oracle_reply"}},
		{"basic-admission-failure", "wrong_model", false, true, []string{"turn_input", "short confirmation"}},
		{"reasoning-success", "history:turn", true, false, []string{"history_tool_input", "history_tool_output", "oracle_reply_fragment", "oracle_result", "Using the price we already checked"}},
		{"reasoning-admission-failure", "history:wrong_model", true, true, []string{"turn_input", "Look up the price per kilogram of apples"}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			options := fixtureOptions(t, tc.mode)
			dir := t.TempDir()
			options.Diagnostics = live.NewDiagnostics(dir, options.APIKey)
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			runner := live.Basic
			if tc.reasoning {
				runner = live.ReasoningHistory
			}
			_, err := runner(ctx, options)
			if (err != nil) != tc.failure {
				t.Fatalf("unexpected outcome: %v", err)
			}
			options.Diagnostics.Close()
			retained := retainedRouteDiagnostics(t, dir)
			for _, want := range tc.wanted {
				if !strings.Contains(retained, want) {
					t.Fatalf("missing retained %q", want)
				}
			}
			if strings.Contains(retained, options.APIKey) {
				t.Fatal("credential retained")
			}
		})
	}
}

func TestEditRoutePersistedDiagnostics(t *testing.T) {
	for _, mode := range []string{"success", "wrong_bytes", "read_error", "continuation_failed", "decline"} {
		t.Run(mode, func(t *testing.T) {
			options := fixtureOptions(t, "edit:"+mode)
			dir := t.TempDir()
			options.Diagnostics = live.NewDiagnostics(dir, options.APIKey)
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			runner := live.StructuredEdit
			if mode == "decline" {
				runner = live.StructuredEditApprovalDeclined
			}
			got, err := runner(ctx, options)
			wantFailure := mode != "success" && mode != "decline"
			if (err != nil) != wantFailure || got.Completed == wantFailure {
				t.Fatalf("outcome changed: %+v %v", got, err)
			}
			options.Diagnostics.Close()
			retained := retainedRouteDiagnostics(t, dir)
			for _, want := range []string{"turn_input", "edit_file", "settled", "before", editSeedToken, "oracle_reply"} {
				if !strings.Contains(retained, want) {
					t.Fatalf("missing persisted %q after %s", want, mode)
				}
			}
			if mode == "decline" && !strings.Contains(retained, "approval_response") {
				t.Fatal("missing actual denial response")
			}
			if mode != "decline" && !strings.Contains(retained, editReplacedToken) {
				t.Fatal("missing changed fixture on success/failure")
			}
			if strings.Contains(retained, options.APIKey) {
				t.Fatal("credential retained")
			}
		})
	}
}

func TestBasicTimeoutRetainsAttemptDiagnostics(t *testing.T) {
	options := fixtureOptions(t, "hang")
	dir := t.TempDir()
	options.Diagnostics = live.NewDiagnostics(dir, options.APIKey)
	ctx, cancel := context.WithTimeout(context.Background(), 500*time.Millisecond)
	defer cancel()
	_, err := live.Basic(ctx, options)
	options.Diagnostics.Close()
	if err == nil {
		t.Fatal("timeout accepted")
	}
	retained := retainedRouteDiagnostics(t, dir)
	if !strings.Contains(retained, "turn_input") || !strings.Contains(retained, "short confirmation") {
		t.Fatal("timeout discarded submitted task")
	}
}

func TestBasicDiagnosticFailureDoesNotChangeOutcome(t *testing.T) {
	options := fixtureOptions(t, "turn")
	blocked := filepath.Join(t.TempDir(), "not-a-directory")
	if err := os.WriteFile(blocked, []byte("occupied"), 0600); err != nil {
		t.Fatal(err)
	}
	options.Diagnostics = live.NewDiagnostics(blocked, options.APIKey)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	got, err := live.Basic(ctx, options)
	options.Diagnostics.Close()
	if err != nil || !got.Completed {
		t.Fatalf("capture failure changed outcome: %+v %v", got, err)
	}
	if len(options.Diagnostics.Status().CaptureErrors) == 0 {
		t.Fatal("capture failure silently lost")
	}
}
