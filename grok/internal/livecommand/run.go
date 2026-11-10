package livecommand

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"io"
	"net"
	"net/url"
	"path/filepath"
	"strings"
	"time"

	"github.com/Harness-X-Harness/codex/grok/dist"
	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/internal/scenarios"
	"github.com/Harness-X-Harness/codex/grok/internal/testkit"
	"github.com/Harness-X-Harness/codex/grok/live"
)

// Report preserves the external grok-live JSON contract.
type Report struct {
	Diagnostics               *live.DiagnosticsStatus `json:"diagnostics,omitempty"`
	ExecutionSnapshotRetained bool                    `json:"execution_snapshot_retained,omitempty"`
	SchemaVersion             int                     `json:"schema_version"`
	Outcome                   string                  `json:"outcome"`
	Reason                    string                  `json:"reason,omitempty"`
	Scenario                  string                  `json:"scenario,omitempty"`
	EndpointSHA256            string                  `json:"endpoint_sha256,omitempty"`
	RecordedAt                string                  `json:"recorded_at"`
	Evidence                  any                     `json:"evidence,omitempty"`
	Failure                   *live.FailureInfo       `json:"failure,omitempty"`
}

const Usage = `Usage: grok-live --scenario NAME --binary PATH --binary-sha256 SHA256
  --source-sha COMMIT --harness-sha COMMIT --target TARGET --environment LABEL
  [--endpoint BASE_URL] [--timeout 5m]

List supported scenarios with grok-scenarios --family live --suite diagnostic.

Execution requires GROK_LIVE=1 and GROK_API_KEY from an approved environment.
Source, target, harness and package eligibility are established by the caller;
the runner verifies copied executable bytes and owns the execution snapshot.
Shipped scenarios use their owned endpoint
and reject an endpoint override. Each selected scenario is invoked once.
Exit codes: 0 observed, 1 not observed/output failure, 2 configuration error,
3 not run without opt-in. --help exits 0 without invoking a scenario.
Formal acceptance requires identified GitHub Actions evidence.
`

func Run(ctx context.Context, args []string, getenv func(string) string, output io.Writer) int {
	finish := func(outcome, reason string, code int) int {
		r := newReport()
		r.Outcome, r.Reason = outcome, reason
		if Write(output, r) != nil {
			return 1
		}
		return code
	}
	flags := flag.NewFlagSet("grok-live", flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	selected := flags.String("scenario", "", "one supported scenario")
	binary := flags.String("binary", "", "explicit source executable")
	digest := flags.String("binary-sha256", "", "verified package runtime digest")
	source := flags.String("source-sha", "", "caller-established source")
	harness := flags.String("harness-sha", "", "caller-established harness")
	target := flags.String("target", "", "supported package target")
	environment := flags.String("environment", "", "execution environment label")
	endpoint := flags.String("endpoint", "", "explicit fixture base URL")
	diagnostics := flags.String("diagnostics-dir", "", "separate bounded task diagnostic directory")
	deadline := flags.Duration("timeout", 5*time.Minute, "whole scenario deadline, at most 10m")
	if err := flags.Parse(args); err != nil {
		if err == flag.ErrHelp {
			if _, err := io.WriteString(output, Usage); err != nil {
				return 1
			}
			return 0
		}
		return finish("configuration_error", "invalid_arguments", 2)
	}
	if flags.NArg() != 0 || *deadline <= 0 || *deadline > 10*time.Minute ||
		(*target != "x86_64-unknown-linux-musl" && *target != "aarch64-apple-darwin") {
		return finish("configuration_error", "invalid_arguments", 2)
	}
	r, code := Execute(ctx, Config{DiagnosticsDir: *diagnostics, Scenario: *selected, Subject: live.Subject{Binary: *binary, SHA256: *digest, SourceSHA: *source, HarnessSHA: *harness, Target: *target, Environment: *environment}, Endpoint: *endpoint, Timeout: *deadline}, getenv)
	if Write(output, r) != nil {
		return 1
	}
	return code
}

// Config binds one catalog scenario to an explicitly selected runtime subject.
type Config struct {
	DiagnosticsDir string
	Scenario       string
	Subject        live.Subject
	Endpoint       string
	Timeout        time.Duration
}

func newReport() Report {
	return Report{SchemaVersion: 1, RecordedAt: time.Now().UTC().Format(time.RFC3339Nano)}
}

// Write emits a single bounded report without serializing raw errors or paths.
func Write(output io.Writer, r Report) error {
	data, err := json.Marshal(r)
	if err != nil || len(data)+1 > 1<<20 {
		return errors.New("bounded evidence encoding failed")
	}
	data = append(data, '\n')
	written, err := output.Write(data)
	if err == nil && written != len(data) {
		return io.ErrShortWrite
	}
	return err
}

// Execute is shared by the compatibility CLI and tagged TestLive subtests.
// It invokes the selected owned scenario once and never retries semantic work.
func Execute(ctx context.Context, config Config, getenv func(string) string) (report Report, code int) {
	r := newReport()
	var diagnostics *live.Diagnostics
	if config.DiagnosticsDir != "" {
		diagnostics = live.NewDiagnostics(config.DiagnosticsDir, getenv("GROK_API_KEY"))
	}
	diagnostics.Record("invocation_started", map[string]any{"scenario": config.Scenario, "source_sha": config.Subject.SourceSHA, "harness_sha": config.Subject.HarnessSHA, "runtime_sha256": config.Subject.SHA256, "model_selection": "catalog", "target": config.Subject.Target, "environment": config.Subject.Environment})
	defer func() {
		diagnostics.Record("invocation_finished", map[string]any{"outcome": report.Outcome, "reason": report.Reason, "failure": report.Failure, "exit_code": code})
		diagnostics.Close()
		if diagnostics != nil {
			status := diagnostics.Status()
			report.Diagnostics = &status
		}
	}()
	finish := func(outcome, reason string, code int) (Report, int) {
		r.Outcome, r.Reason = outcome, reason
		return r, code
	}
	if config.Timeout <= 0 || config.Timeout > 10*time.Minute {
		return finish("configuration_error", "invalid_arguments", 2)
	}
	scenario, err := scenarios.Lookup("live", config.Scenario)
	if err != nil {
		return finish("configuration_error", "unknown_or_missing_scenario", 2)
	}
	r.Scenario = scenario.Name
	if diagnostics == nil && getenv("GROK_LIVE_DIAGNOSTICS_DIR") != "" {
		diagnostics = live.NewDiagnostics(filepath.Join(getenv("GROK_LIVE_DIAGNOSTICS_DIR"), scenario.Name), getenv("GROK_API_KEY"))
		diagnostics.Record("invocation_started", map[string]any{"scenario": scenario.Name, "model": scenario.Model, "source_sha": config.Subject.SourceSHA, "harness_sha": config.Subject.HarnessSHA, "runtime_sha256": config.Subject.SHA256, "target": config.Subject.Target, "environment": config.Subject.Environment})
	}
	if getenv("GROK_LIVE") != "1" {
		return finish("not_run", "explicit_opt_in_required", 3)
	}
	key := strings.TrimSpace(getenv("GROK_API_KEY"))
	if key == "" {
		return finish("configuration_error", "credential_required", 2)
	}
	if strings.HasPrefix(scenario.Name, "shipped-") {
		if config.Endpoint != "" {
			return finish("configuration_error", "shipped_endpoint_is_owned_by_profile", 2)
		}
		_, base, err := dist.Defaults()
		if err != nil {
			return finish("configuration_error", "invalid_shipped_profile", 2)
		}
		config.Endpoint = base
	} else if config.Endpoint == "" {
		config.Endpoint = strings.TrimSuffix(providerfixture.ResponsesEndpoint, "/responses")
	}
	u, err := url.Parse(config.Endpoint)
	if err != nil || u.Hostname() == "" || u.User != nil || u.RawQuery != "" || u.Fragment != "" {
		return finish("configuration_error", "invalid_endpoint", 2)
	}
	ip := net.ParseIP(u.Hostname())
	if u.Scheme != "https" && (u.Scheme != "http" || ip == nil || !ip.IsLoopback()) {
		return finish("configuration_error", "https_or_loopback_endpoint_required", 2)
	}
	endpointDigest := sha256.Sum256([]byte(config.Endpoint))
	r.EndpointSHA256 = hex.EncodeToString(endpointDigest[:])
	ctx, cancel := context.WithTimeout(ctx, config.Timeout)
	defer cancel()
	// Keep subject provenance and digest unchanged, but bind execution to a
	// private snapshot so rebuilding/replacing the input path cannot change
	// the runtime or its package-relative helpers during this observation.
	binary, cleanup, err := testkit.SnapshotExecutable(config.Subject.Binary, config.Subject.SHA256)
	if err != nil {
		return finish("configuration_error", "subject_or_fixture_not_verified", 2)
	}
	// Default to retaining on panic or any path without an explicit join proof.
	retainSnapshot := true
	defer func() {
		if !retainSnapshot {
			cleanup()
		}
	}()
	config.Subject.Binary = binary
	options := live.Options{Diagnostics: diagnostics, Subject: config.Subject, Model: scenario.Model, BaseURL: config.Endpoint, APIKey: key}
	result, err := scenario.RunLive(ctx, options)
	r.Evidence = result.Evidence
	evidence := result.Base
	retainSnapshot = evidence.CleanupPending
	r.ExecutionSnapshotRetained = retainSnapshot
	complete := evidence.Completed || (scenario.Name == "shipped-catalog" && evidence.ShippedCatalog)
	if err != nil || !complete || evidence.Turns > scenario.SemanticTurns {
		reason := "scenario_not_completed"
		switch {
		case ctx.Err() == context.DeadlineExceeded:
			reason = "deadline_exceeded"
		case ctx.Err() == context.Canceled:
			reason = "cancelled"
		case evidence.Stage == "preflight":
			return finish("configuration_error", "subject_or_fixture_not_verified", 2)
		}
		if err != nil {
			detail := live.DescribeFailure(err)
			r.Failure = &detail
		}
		return finish("not_observed", reason, 1)
	}
	return finish("observed", "", 0)
}
