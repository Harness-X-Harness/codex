package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"io"
	"net"
	"net/url"
	"strings"
	"time"

	"github.com/Harness-X-Harness/codex/grok/dist"
	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

type report struct {
	SchemaVersion  int    `json:"schema_version"`
	Outcome        string `json:"outcome"`
	Reason         string `json:"reason,omitempty"`
	Scenario       string `json:"scenario,omitempty"`
	EndpointSHA256 string `json:"endpoint_sha256,omitempty"`
	RecordedAt     string `json:"recorded_at"`
	Evidence       any    `json:"evidence,omitempty"`
}

const usage = `Usage: grok-live --scenario NAME --binary PATH --binary-sha256 SHA256
  --source-sha COMMIT --harness-sha COMMIT --target TARGET --environment LABEL
  [--endpoint BASE_URL] [--timeout 5m]

Scenarios: basic-primary, basic-pinned, reasoning-primary, reasoning-pinned,
structured-edit, structured-edit-declined, structured-edit-pinned,
structured-edit-all, image-primary, image-pinned, shipped-catalog,
shipped-startup, shipped-pinned, shipped-child, web, web-allowed,
web-excluded, x, x-window.

Execution requires GROK_LIVE=1 and GROK_API_KEY from an approved environment.
Source, target, harness and package eligibility are established by the caller;
the runner verifies the binary digest. Shipped scenarios use their owned endpoint
and reject an endpoint override. Each selected scenario is invoked once.
Exit codes: 0 observed, 1 not observed/output failure, 2 configuration error,
3 not run without opt-in. --help exits 0 without invoking a scenario.
Formal acceptance requires identified GitHub Actions evidence.
`

func run(ctx context.Context, args []string, getenv func(string) string, output io.Writer) int {
	r := report{SchemaVersion: 1, RecordedAt: time.Now().UTC().Format(time.RFC3339Nano)}
	finish := func(outcome, reason string, code int) int {
		r.Outcome, r.Reason = outcome, reason
		if json.NewEncoder(output).Encode(r) != nil {
			return 1
		}
		return code
	}
	flags := flag.NewFlagSet("grok-live", flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	selected := flags.String("scenario", "", "one supported scenario")
	binary := flags.String("binary", "", "explicit immutable runtime")
	digest := flags.String("binary-sha256", "", "verified package runtime digest")
	source := flags.String("source-sha", "", "caller-established source")
	harness := flags.String("harness-sha", "", "caller-established harness")
	target := flags.String("target", "", "supported package target")
	environment := flags.String("environment", "", "execution environment label")
	endpoint := flags.String("endpoint", "", "explicit fixture base URL")
	deadline := flags.Duration("timeout", 5*time.Minute, "whole scenario deadline, at most 10m")
	if err := flags.Parse(args); err != nil {
		if err == flag.ErrHelp {
			if _, err := io.WriteString(output, usage); err != nil {
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
	switch *selected {
	case "basic-primary", "basic-pinned", "reasoning-primary", "reasoning-pinned", "structured-edit", "structured-edit-declined", "structured-edit-pinned", "structured-edit-all", "image-primary", "image-pinned", "shipped-catalog", "shipped-startup", "shipped-pinned", "shipped-child", "web", "web-allowed", "web-excluded", "x", "x-window":
	default:
		return finish("configuration_error", "unknown_or_missing_scenario", 2)
	}
	r.Scenario = *selected
	if getenv("GROK_LIVE") != "1" {
		return finish("not_run", "explicit_opt_in_required", 3)
	}
	key := strings.TrimSpace(getenv("GROK_API_KEY"))
	if key == "" {
		return finish("configuration_error", "credential_required", 2)
	}
	if strings.HasPrefix(*selected, "shipped-") {
		if *endpoint != "" {
			return finish("configuration_error", "shipped_endpoint_is_owned_by_profile", 2)
		}
		_, base, err := dist.Defaults()
		if err != nil {
			return finish("configuration_error", "invalid_shipped_profile", 2)
		}
		*endpoint = base
	} else if *endpoint == "" {
		*endpoint = strings.TrimSuffix(providerfixture.ResponsesEndpoint, "/responses")
	}
	u, err := url.Parse(*endpoint)
	if err != nil || u.Hostname() == "" || u.User != nil || u.RawQuery != "" || u.Fragment != "" {
		return finish("configuration_error", "invalid_endpoint", 2)
	}
	ip := net.ParseIP(u.Hostname())
	if u.Scheme != "https" && (u.Scheme != "http" || ip == nil || !ip.IsLoopback()) {
		return finish("configuration_error", "https_or_loopback_endpoint_required", 2)
	}
	endpointDigest := sha256.Sum256([]byte(*endpoint))
	r.EndpointSHA256 = hex.EncodeToString(endpointDigest[:])
	subject := live.Subject{Binary: *binary, SHA256: *digest, SourceSHA: *source, HarnessSHA: *harness, Target: *target, Environment: *environment}
	options := live.Options{Subject: subject, Model: providerfixture.PrimaryModel, BaseURL: *endpoint, APIKey: key}
	if strings.HasSuffix(*selected, "-pinned") {
		options.Model = providerfixture.PinnedModel
	}
	ctx, cancel := context.WithTimeout(ctx, *deadline)
	defer cancel()
	var evidence live.Evidence
	switch *selected {
	case "basic-primary", "basic-pinned":
		evidence, err = live.Basic(ctx, options)
		r.Evidence = evidence
	case "reasoning-primary", "reasoning-pinned":
		evidence, err = live.ReasoningHistory(ctx, options)
		r.Evidence = evidence
	case "structured-edit", "structured-edit-declined", "structured-edit-pinned", "structured-edit-all":
		runner := live.StructuredEdit
		switch *selected {
		case "structured-edit-declined":
			runner = live.StructuredEditApprovalDeclined
		case "structured-edit-pinned":
			runner = live.StructuredEditPinnedPreviousModel
		case "structured-edit-all":
			runner = live.StructuredEditReplaceAll
		}
		observed, failure := runner(ctx, options)
		r.Evidence, evidence, err = observed, observed.Evidence, failure
	case "image-primary", "image-pinned":
		observed, failure := live.ImageGenerationEdit(ctx, options)
		r.Evidence, evidence, err = observed, observed.Evidence, failure
	case "shipped-catalog", "shipped-startup", "shipped-pinned", "shipped-child":
		runner := live.ShippedCatalog
		switch *selected {
		case "shipped-startup":
			runner = live.ShippedStartup
		case "shipped-pinned":
			runner = live.ShippedPinned
		case "shipped-child":
			runner = live.ShippedChildCollaboration
		}
		evidence, err = runner(ctx, subject, key)
		r.Evidence = evidence
	default:
		observed, failure := live.HostedSearch(ctx, options, strings.ReplaceAll(*selected, "-", "_"))
		r.Evidence, evidence, err = observed, observed.Evidence, failure
	}
	complete := evidence.Completed || (*selected == "shipped-catalog" && evidence.ShippedCatalog)
	if err != nil || !complete {
		reason := "scenario_not_completed"
		switch {
		case ctx.Err() == context.DeadlineExceeded:
			reason = "deadline_exceeded"
		case ctx.Err() == context.Canceled:
			reason = "cancelled"
		case evidence.Stage == "preflight":
			return finish("configuration_error", "subject_or_fixture_not_verified", 2)
		}
		return finish("not_observed", reason, 1)
	}
	return finish("observed", "", 0)
}
