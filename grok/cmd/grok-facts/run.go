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
	"regexp"
	"strings"
	"time"

	"github.com/Harness-X-Harness/codex/grok/dist"
	"github.com/Harness-X-Harness/codex/grok/facts"
	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
)

const usage = `Usage: grok-facts --scenario NAME --source-sha COMMIT [--endpoint URL] [--timeout 2m]

Scenarios: basic-primary, basic-pinned, encrypted-replay, shipped-routes,
web, web-allowed, web-excluded, x, x-window, web-replay, x-replay.

Execution requires GROK_FACTS=1 and GROK_API_KEY in the environment.
One JSON report is written to stdout. Exit codes: 0 observed, 1 not observed
or output failure, 2 invalid configuration, 3 not run without explicit opt-in.
--help exits 0 without an observation report or any backend invocation.

The source SHA is a caller declaration, not independently established provenance.
Reports contain no endpoint, credential, response text, ciphertext or opaque ID.
An observation is not a product Live result, native admission or a freshness claim
for another execution. Formal acceptance requires the GitHub Actions evidence.
`

type report struct {
	SchemaVersion     int                      `json:"schema_version"`
	Outcome           string                   `json:"outcome"`
	Reason            string                   `json:"reason,omitempty"`
	Scenario          string                   `json:"scenario,omitempty"`
	DeclaredSourceSHA string                   `json:"declared_source_sha,omitempty"`
	EndpointSHA256    string                   `json:"endpoint_sha256,omitempty"`
	RecordedAt        string                   `json:"recorded_at"`
	Observation       *facts.Observation       `json:"observation,omitempty"`
	Search            *facts.SearchObservation `json:"search,omitempty"`
	Routes            []facts.RouteObservation `json:"routes,omitempty"`
}

func run(ctx context.Context, args []string, getenv func(string) string, output io.Writer) int {
	r := report{SchemaVersion: 1, RecordedAt: time.Now().UTC().Format(time.RFC3339Nano)}
	finish := func(outcome, reason string, code int) int {
		r.Outcome, r.Reason = outcome, reason
		if json.NewEncoder(output).Encode(r) != nil {
			return 1
		}
		return code
	}
	flags := flag.NewFlagSet("grok-facts", flag.ContinueOnError)
	// flag errors may contain arbitrary user input, including endpoint secrets.
	flags.SetOutput(io.Discard)
	selected := flags.String("scenario", "", "one supported observation")
	source := flags.String("source-sha", "", "caller-declared source commit")
	endpoint := flags.String("endpoint", "", "explicit Responses endpoint")
	deadline := flags.Duration("timeout", 2*time.Minute, "whole observation deadline, at most 3m")
	if err := flags.Parse(args); err != nil {
		if err == flag.ErrHelp {
			if _, err := io.WriteString(output, usage); err != nil {
				return 1
			}
			return 0
		}
		return finish("configuration_error", "invalid_arguments", 2)
	}
	if flags.NArg() != 0 || !regexp.MustCompile(`^[0-9a-f]{40}$`).MatchString(*source) || *deadline <= 0 || *deadline > 3*time.Minute {
		return finish("configuration_error", "invalid_arguments", 2)
	}
	switch *selected {
	case "basic-primary", "basic-pinned", "encrypted-replay", "shipped-routes", "web", "web-allowed", "web-excluded", "x", "x-window", "web-replay", "x-replay":
	default:
		return finish("configuration_error", "unknown_or_missing_scenario", 2)
	}
	r.Scenario, r.DeclaredSourceSHA = *selected, *source
	if getenv("GROK_FACTS") != "1" {
		return finish("not_run", "explicit_opt_in_required", 3)
	}
	key := strings.TrimSpace(getenv("GROK_API_KEY"))
	if key == "" {
		return finish("configuration_error", "credential_required", 2)
	}
	if *endpoint == "" {
		*endpoint = providerfixture.ResponsesEndpoint
		if *selected == "shipped-routes" {
			_, base, err := dist.Defaults()
			if err != nil {
				return finish("configuration_error", "invalid_shipped_profile", 2)
			}
			*endpoint = base + "/responses"
		}
	}
	u, err := url.Parse(*endpoint)
	if err != nil || u.Hostname() == "" || u.User != nil || u.RawQuery != "" || u.Fragment != "" {
		return finish("configuration_error", "invalid_endpoint", 2)
	}
	ip := net.ParseIP(u.Hostname())
	if u.Scheme != "https" && (u.Scheme != "http" || ip == nil || !ip.IsLoopback()) {
		return finish("configuration_error", "https_or_loopback_endpoint_required", 2)
	}
	digest := sha256.Sum256([]byte(*endpoint))
	r.EndpointSHA256 = hex.EncodeToString(digest[:])
	probe, err := facts.NewProbe(*endpoint, key)
	if err != nil {
		return finish("configuration_error", "invalid_probe_configuration", 2)
	}
	ctx, cancel := context.WithTimeout(ctx, *deadline)
	defer cancel()
	var observed facts.Observation
	switch *selected {
	case "basic-primary":
		observed, err = probe.Text(ctx, providerfixture.PrimaryModel)
		r.Observation = &observed
	case "basic-pinned":
		observed, err = probe.Text(ctx, providerfixture.PinnedModel)
		r.Observation = &observed
	case "encrypted-replay":
		observed, err = probe.EncryptedReplay(ctx, providerfixture.PrimaryModel)
		r.Observation = &observed
	case "shipped-routes":
		r.Routes, err = probe.ShippedRoutes(ctx)
		if len(r.Routes) != 0 {
			observed = r.Routes[len(r.Routes)-1].Observation
		}
	default:
		var search facts.SearchObservation
		if strings.HasSuffix(*selected, "-replay") {
			search, err = probe.SearchReplay(ctx, providerfixture.PrimaryModel, strings.TrimSuffix(*selected, "-replay"))
		} else {
			search, err = probe.SearchPolicy(ctx, providerfixture.PrimaryModel, strings.ReplaceAll(*selected, "-", "_"))
		}
		r.Search, observed = &search, search.Observation
	}
	if err != nil || !observed.Completed {
		reason := "probe_did_not_complete"
		switch {
		case ctx.Err() == context.DeadlineExceeded:
			reason = "deadline_exceeded"
		case ctx.Err() == context.Canceled:
			reason = "cancelled"
		case observed.HTTPStatus == 401 || observed.HTTPStatus == 403:
			reason = "authentication_or_access_rejected"
		case observed.HTTPStatus != 0 && (observed.HTTPStatus < 200 || observed.HTTPStatus >= 300):
			reason = "http_rejected"
		}
		return finish("not_observed", reason, 1)
	}
	return finish("observed", "", 0)
}
