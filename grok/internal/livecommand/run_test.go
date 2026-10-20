package livecommand

import (
	"bytes"
	"context"
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
