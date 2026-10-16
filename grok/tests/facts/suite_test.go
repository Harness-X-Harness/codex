//go:build facts

// Package facts_test runs explicitly selected, bounded direct Responses probes.
// This is backend-only evidence, independent of Codex binary/product support.
package facts_test

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"io"
	"net"
	"net/url"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/dist"
	"github.com/Harness-X-Harness/codex/grok/facts"
	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/internal/scenarios"
)

var (
	suite     = flag.String("grok-facts-suite", "full", "full, smoke or diagnostic")
	scenario  = flag.String("grok-facts-scenario", "", "optional exact Facts catalog name; -run also selects subtests")
	endpoint  = flag.String("grok-facts-endpoint", "", "optional explicit HTTPS or loopback Responses endpoint")
	deadline  = flag.Duration("grok-facts-timeout", 3*time.Minute, "per-scenario timeout, at most 3m")
	cases     []scenarios.Scenario
	selected  int
	completed int
)

// TestMain prevents missing, skipped, repeated or incomplete selections from
// earning backend evidence. -list remains credential-free discovery only.
func TestMain(m *testing.M) {
	flag.CommandLine.SetOutput(io.Discard)
	flag.Parse()
	if flag.Lookup("test.list").Value.String() != "" {
		os.Exit(m.Run())
	}
	reason := configure()
	code := 1
	if reason == "" {
		code = m.Run()
		if selected == 0 || completed != selected {
			reason = "empty_or_incomplete_selection"
			code = 1
		}
	}
	outcome := "not_observed"
	if code == 0 {
		outcome = "observed"
	}
	// Never retain raw flags, endpoints, keys, responses or opaque replay IDs.
	report := struct {
		Family    string `json:"family"`
		Scope     string `json:"scope"`
		Outcome   string `json:"outcome"`
		Reason    string `json:"reason,omitempty"`
		Selected  int    `json:"selected"`
		Completed int    `json:"completed"`
	}{"facts", "direct_backend", outcome, reason, selected, completed}
	if json.NewEncoder(os.Stdout).Encode(report) != nil {
		code = 1
	}
	os.Exit(code)
}

func configure() string {
	if *suite == "diagnostic" && *scenario == "" {
		return "diagnostic_scenario_required"
	}
	var err error
	cases, err = scenarios.Select("facts", *suite, *scenario)
	if err != nil {
		return "invalid_scenario_selection"
	}
	explicitCount := false
	flag.Visit(func(f *flag.Flag) {
		if f.Name == "test.count" {
			explicitCount = true
		}
	})
	if !explicitCount {
		return "uncached_execution_required"
	}
	if flag.Lookup("test.count").Value.String() != "1" || strings.Contains(flag.Lookup("test.cpu").Value.String(), ",") {
		return "repeated_execution_forbidden"
	}
	if flag.Lookup("test.skip").Value.String() != "" {
		return "skipped_selection_forbidden"
	}
	if *deadline <= 0 || *deadline > 3*time.Minute {
		return "invalid_scenario_timeout"
	}
	if *endpoint != "" {
		u, err := url.Parse(*endpoint)
		if err != nil || u.Hostname() == "" || u.User != nil || u.RawQuery != "" || u.Fragment != "" {
			return "invalid_endpoint"
		}
		ip := net.ParseIP(u.Hostname())
		if u.Scheme != "https" && (u.Scheme != "http" || ip == nil || !ip.IsLoopback()) {
			return "https_or_loopback_endpoint_required"
		}
	}
	if os.Getenv("GROK_FACTS") != "1" {
		return "explicit_opt_in_required"
	}
	if strings.TrimSpace(os.Getenv("GROK_API_KEY")) == "" {
		return "credential_required"
	}
	return ""
}

func searchFact(scenario scenarios.Scenario) bool {
	return strings.HasPrefix(scenario.Policy, "web") || strings.HasPrefix(scenario.Policy, "x")
}

func observe(t *testing.T, scenario scenarios.Scenario) {
	t.Helper()
	selected++
	address := *endpoint
	if address == "" {
		address = providerfixture.ResponsesEndpoint
		if scenario.Policy == "shipped-routes" {
			_, base, err := dist.Defaults()
			if err != nil {
				t.Fatal("invalid_shipped_profile")
			}
			address = base + "/responses"
		}
	}
	probe, err := facts.NewProbe(address, strings.TrimSpace(os.Getenv("GROK_API_KEY")))
	if err != nil {
		t.Fatal("invalid_probe_configuration")
	}
	timeout := *deadline
	if !searchFact(scenario) || scenario.SemanticTurns == 1 {
		timeout = min(timeout, 2*time.Minute)
	}
	if end, ok := t.Deadline(); ok {
		timeout = min(timeout, time.Until(end)-5*time.Second)
	}
	if timeout <= 0 {
		t.Fatal("scenario_timeout_budget_exhausted")
	}
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()
	result, err := scenario.RunFacts(ctx, probe)
	encoded, encodeErr := json.Marshal(result)
	if encodeErr != nil || len(encoded) > 1<<20 {
		t.Fatal("bounded_evidence_write_failed")
	}
	digest := sha256.Sum256([]byte(address))
	t.Logf("BACKEND_FACT observed_at=%s endpoint_sha256=%s metadata=%s", time.Now().UTC().Format(time.RFC3339), hex.EncodeToString(digest[:]), encoded)
	if err != nil || !result.Completed() {
		t.Fatal("NOT_OBSERVED reason=incomplete_backend_fact")
	}
	requests := result.LastObservation().Requests
	if len(result.Routes) != 0 {
		requests = 0
		for _, route := range result.Routes {
			requests += route.Observation.Requests
		}
	}
	if requests != scenario.SemanticTurns {
		t.Fatal("NOT_OBSERVED reason=request_budget_mismatch")
	}
	completed++
}
