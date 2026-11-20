//go:build live

// Package live_test runs opt-in product-to-backend scenarios. Its evidence is
// separate from both controlled Integration and direct HTTP Facts.
package live_test

import (
	"bytes"
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"regexp"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/livecommand"
	"github.com/Harness-X-Harness/codex/grok/internal/scenarios"
	"github.com/Harness-X-Harness/codex/grok/internal/testkit"
	"github.com/Harness-X-Harness/codex/grok/live"
)

var (
	suite                         = flag.String("grok-live-suite", "smoke", "smoke, full or diagnostic")
	scenario                      = flag.String("grok-live-scenario", "", "optional exact catalog name; -run also selects subtests")
	verified                      = flag.Bool("grok-live-verified-subject", false, "caller established exact package provenance")
	digest                        = flag.String("grok-live-binary-sha256", "", "verified package runtime digest")
	source                        = flag.String("grok-live-source-sha", "", "verified immutable runtime source revision")
	harness                       = flag.String("grok-live-harness-sha", "", "verified immutable harness revision")
	target                        = flag.String("grok-live-target", "", "verified package target")
	environment                   = flag.String("grok-live-environment", "", "verified execution environment label")
	deadline                      = flag.Duration("grok-live-timeout", 5*time.Minute, "per-scenario timeout, at most 5m")
	cases                         []scenarios.Scenario
	subject                       live.Subject
	selectedCount, completedCount int
)

// TestMain refuses a green package when -run/-skip selects no scenarios, or a
// selected case does not finish. Discovery (-list) never requires credentials.
func TestMain(m *testing.M) {
	// Flag errors may contain arbitrary input, including private file names.
	flag.CommandLine.SetOutput(io.Discard)
	flag.Parse()
	if flag.Lookup("test.list").Value.String() != "" {
		os.Exit(m.Run())
	}
	failure := configure()
	code := 1
	if failure == "" {
		code = m.Run()
	}
	if failure == "" && (selectedCount == 0 || completedCount != selectedCount) {
		failure = "empty_or_incomplete_selection"
		code = 1
	}
	outcome := "not_proven"
	if code == 0 && failure == "" {
		outcome = "observed"
	}
	// Only validated catalog/configuration labels enter the retained summary.
	summary := struct {
		SchemaVersion int    `json:"schema_version"`
		Family        string `json:"family"`
		Suite         string `json:"suite,omitempty"`
		Outcome       string `json:"outcome"`
		Reason        string `json:"reason,omitempty"`
		Selected      int    `json:"selected"`
		Completed     int    `json:"completed"`
	}{SchemaVersion: 1, Family: "live", Outcome: outcome, Reason: failure, Selected: selectedCount, Completed: completedCount}
	if len(cases) != 0 {
		summary.Suite = *suite
	}
	data, err := json.Marshal(summary)
	if err != nil || len(data) > 1<<20 {
		os.Exit(1)
	}
	if _, err := fmt.Fprintln(os.Stdout, string(data)); err != nil {
		code = 1
	}
	os.Exit(code)
}

func configure() string {
	if *suite == "diagnostic" && *scenario == "" {
		return "diagnostic_scenario_required"
	}
	var err error
	cases, err = scenarios.Select("live", *suite, *scenario)
	if err != nil {
		return "invalid_scenario_selection"
	}
	// Explicit count=1 both prevents cached backend success and disallows paid
	// semantic resubmission under one invocation.
	explicitCount := false
	flag.Visit(func(f *flag.Flag) {
		if f.Name == "test.count" {
			explicitCount = true
		}
	})
	if !explicitCount {
		return "uncached_execution_required"
	}
	// Go repetitions would resubmit paid semantic work under one invocation.
	if flag.Lookup("test.count").Value.String() != "1" || strings.Contains(flag.Lookup("test.cpu").Value.String(), ",") {
		return "repeated_execution_forbidden"
	}
	// Use -run for an explicit subset. -skip must never silently remove a
	// required owner from a selected smoke/full suite.
	if flag.Lookup("test.skip").Value.String() != "" {
		return "skipped_selection_forbidden"
	}
	if *deadline <= 0 || *deadline > 5*time.Minute {
		return "invalid_scenario_timeout"
	}
	if os.Getenv("GROK_LIVE") != "1" {
		return "explicit_opt_in_required"
	}
	if strings.TrimSpace(os.Getenv("GROK_API_KEY")) == "" {
		return "credential_required"
	}
	binary, err := testkit.SelectBinary(os.Getenv)
	if err != nil {
		return "binary_selection_invalid"
	}
	actual, err := testkit.ExecutableSHA256(binary)
	if err != nil {
		return "binary_unavailable"
	}
	subject = live.Subject{Binary: binary, SHA256: actual, Target: runtime.GOOS + "-" + runtime.GOARCH, Environment: "local-live", LocalUnverified: true}
	if !*verified {
		if *digest != "" || *source != "" || *harness != "" || *target != "" || *environment != "" {
			return "verified_metadata_requires_explicit_subject"
		}
		return ""
	}
	revision := regexp.MustCompile(`^[0-9a-f]{40}$`)
	label := regexp.MustCompile(`^[a-zA-Z0-9_.-]{1,64}$`)
	if *digest != actual || !revision.MatchString(*source) || !revision.MatchString(*harness) ||
		(*target != "x86_64-unknown-linux-musl" && *target != "aarch64-apple-darwin") || !label.MatchString(*environment) {
		return "verified_subject_invalid"
	}
	subject = live.Subject{Binary: binary, SHA256: actual, SourceSHA: *source, HarnessSHA: *harness, Target: *target, Environment: *environment}
	return ""
}

func TestLive(t *testing.T) {
	for _, scenario := range cases {
		t.Run(scenario.Name, func(t *testing.T) {
			// Intentionally sequential: one process and one owned invocation per case.
			selectedCount++
			timeout := *deadline
			if end, ok := t.Deadline(); ok {
				// Leave cleanup time before Go's timeout panic can print stacks.
				timeout = min(timeout, time.Until(end)-5*time.Second)
			}
			if timeout <= 0 {
				t.Fatal("scenario_timeout_budget_exhausted")
			}
			report, code := livecommand.Execute(context.Background(), livecommand.Config{Scenario: scenario.Name, Subject: subject, Timeout: timeout}, os.Getenv)
			var evidence bytes.Buffer
			if err := livecommand.Write(&evidence, report); err != nil {
				t.Fatal("bounded_evidence_write_failed")
			}
			t.Log(strings.TrimSpace(evidence.String()))
			if code != 0 || report.Outcome != "observed" {
				t.Fatal("scenario_not_proven")
			}
			completedCount++
		})
	}
}
