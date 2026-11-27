package livecommand

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestExecuteRejectsMissingOptInBeforeInvocation(t *testing.T) {
	report, code := Execute(context.Background(), Config{Scenario: "basic-primary", Timeout: time.Minute}, func(string) string { return "" })
	if code != 3 || report.Outcome != "not_run" || report.Reason != "explicit_opt_in_required" || report.Evidence != nil {
		t.Fatal("missing opt-in executed a scenario")
	}
	report, code = Execute(context.Background(), Config{Scenario: "PRIVATE_UNKNOWN", Timeout: time.Minute}, func(string) string { return "PRIVATE_CANARY" })
	var output bytes.Buffer
	if Write(&output, report) != nil || code != 2 || strings.Contains(output.String(), "PRIVATE_") {
		t.Fatal("invalid selection leaked private input")
	}
}

func TestReportHardBoundRejectsOversizedEvidence(t *testing.T) {
	var output bytes.Buffer
	if Write(&output, Report{Evidence: strings.Repeat("x", 1<<20)}) == nil || output.Len() != 0 {
		t.Fatal("oversized report was partially emitted")
	}
	if Write(&output, Report{Evidence: make(chan int)}) == nil || output.Len() != 0 {
		t.Fatal("invalid evidence was emitted")
	}
	if Write(&output, Report{SchemaVersion: 1, Outcome: "not_run"}) != nil || output.Len() == 0 {
		t.Fatal("bounded report was lost")
	}
}

func TestDiagnosticCaptureFailureIsSeparateFromNotRun(t *testing.T) {
	path := filepath.Join(t.TempDir(), "blocked")
	if err := os.WriteFile(path, []byte("not a directory"), 0600); err != nil {
		t.Fatal(err)
	}
	report, code := Execute(context.Background(), Config{Scenario: "basic-primary", Timeout: time.Minute, DiagnosticsDir: path}, func(string) string { return "" })
	if code != 3 || report.Outcome != "not_run" {
		t.Fatalf("diagnostics changed outcome: %+v", report)
	}
	if report.Diagnostics == nil || len(report.Diagnostics.CaptureErrors) == 0 {
		t.Fatalf("capture failure hidden: %+v", report)
	}
	if report.Diagnostics.Invocation == nil || report.Diagnostics.Invocation.Outcome != report.Outcome || report.Diagnostics.Invocation.ExitCode != code {
		t.Fatal("final report lost outcome when diagnostic files were unavailable")
	}
}

func TestDiagnosticEarlyReturnPersistsInvocation(t *testing.T) {
	dir := t.TempDir()
	report, code := Execute(context.Background(), Config{Scenario: "basic-primary", Timeout: time.Minute, DiagnosticsDir: dir}, func(string) string { return "" })
	if code != 3 || report.Diagnostics == nil {
		t.Fatal("not-run outcome changed")
	}
	if report.Diagnostics.Invocation == nil || report.Diagnostics.Invocation.Outcome != report.Outcome || report.Diagnostics.Invocation.Reason != report.Reason || report.Failure != nil || report.Diagnostics.Invocation.Failure.Code != "" {
		t.Fatal("not-run summary disagreed with the final Report or invented failure")
	}
	data, err := os.ReadFile(filepath.Join(dir, "events.jsonl"))
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{"invocation_started", "invocation_finished", "explicit_opt_in_required"} {
		if !strings.Contains(string(data), want) {
			t.Errorf("missing %s", want)
		}
	}
}
