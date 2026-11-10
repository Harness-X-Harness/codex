package live_test

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/live"
)

func TestDiagnosticsLossDoesNotChangeSuccessOrFailure(t *testing.T) {
	for _, mode := range []string{"turn", "failed"} {
		t.Run(mode, func(t *testing.T) {
			var expected live.Evidence
			var expectedFailure live.FailureInfo
			for _, sink := range []string{"disabled", "closed", "unwritable", "flooded"} {
				t.Run(sink, func(t *testing.T) {
					options := fixtureOptions(t, mode)
					if sink != "disabled" {
						path := t.TempDir()
						if sink == "unwritable" {
							path = filepath.Join(path, "occupied")
							if err := os.WriteFile(path, []byte("not a directory"), 0600); err != nil {
								t.Fatal(err)
							}
						}
						options.Diagnostics = live.NewDiagnostics(path, options.APIKey)
						if sink == "closed" {
							options.Diagnostics.Close()
						}
						if sink == "flooded" {
							for i := 0; i < 5000; i++ {
								options.Diagnostics.Record("public_event", map[string]any{"method": "item/agentMessage/delta", "delta": "auxiliary text"})
								options.Diagnostics.Record("auxiliary_noise", nil)
							}
						}
					}
					ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
					defer cancel()
					got, err := live.Basic(ctx, options)
					options.Diagnostics.Close()
					failure := live.DescribeFailure(err)
					if (err == nil) != (mode == "turn") || got.Completed != (mode == "turn") || got.CleanupPending {
						t.Fatalf("unexpected task outcome: completed=%t failure=%+v pending=%t", got.Completed, failure, got.CleanupPending)
					}
					got.ObservedAt = ""
					if sink == "disabled" {
						expected, expectedFailure = got, failure
					} else if got != expected || failure != expectedFailure {
						t.Fatalf("diagnostics altered business evidence: got=%+v failure=%+v", got, failure)
					}
					if sink == "unwritable" || sink == "flooded" {
						status := options.Diagnostics.Status()
						if len(status.CaptureErrors) == 0 || status.Result == nil || status.Result.Completed != got.Completed || status.Result.Failure != failure || status.Cleanup == nil || !status.Cleanup.ProcessReaped || status.Cleanup.CallbackWorkPending != 0 {
							t.Fatalf("optional evidence loss also hid final safe facts: %+v", status)
						}
					}
				})
			}
		})
	}
}
