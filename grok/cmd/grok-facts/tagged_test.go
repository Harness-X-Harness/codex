package main

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func tagged(t *testing.T, args []string, optIn, key string) (string, int) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	command := exec.CommandContext(ctx, taggedPath, args...)
	for _, value := range os.Environ() {
		if !strings.HasPrefix(value, "GROK_FACTS=") && !strings.HasPrefix(value, "GROK_API_KEY=") {
			command.Env = append(command.Env, value)
		}
	}
	command.Env = append(command.Env, "GROK_FACTS="+optIn, "GROK_API_KEY="+key)
	var stdout, stderr bytes.Buffer
	command.Stdout, command.Stderr = &stdout, &stderr
	err := command.Run()
	code := 0
	if err != nil {
		exit, ok := err.(*exec.ExitError)
		if !ok || ctx.Err() != nil {
			t.Fatalf("tagged Facts failed outside result contract: %v", err)
		}
		code = exit.ExitCode()
	}
	if (stderr.Len() != 0 && stderr.String() != "testing: warning: no tests to run\n") || strings.Contains(stdout.String()+stderr.String(), privateMarker) || stdout.Len() > 1<<20 {
		t.Fatal("tagged Facts leaked private input or unbounded evidence")
	}
	return stdout.String(), code
}

func TestTaggedFactsRejectUnprovenSelectionsWithoutRequests(t *testing.T) {
	var requests atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		w.WriteHeader(http.StatusUnauthorized)
	}))
	defer server.Close()
	for _, tc := range []struct {
		name, optIn, key, reason string
		args                     []string
	}{
		{"missing-opt-in", "0", privateMarker, "explicit_opt_in_required", nil},
		{"missing-key", "1", "", "credential_required", nil},
		{"unknown-scenario", "1", privateMarker, "invalid_scenario_selection", []string{"-grok-facts-scenario=" + privateMarker}},
		{"unknown-suite", "1", privateMarker, "invalid_scenario_selection", []string{"-grok-facts-suite=" + privateMarker}},
		{"diagnostic-requires-choice", "1", privateMarker, "diagnostic_scenario_required", []string{"-grok-facts-suite=diagnostic"}},
		{"unmatched-root", "1", privateMarker, "empty_or_incomplete_selection", []string{"-test.run=^Absent$"}},
		{"unmatched-subtest", "1", privateMarker, "empty_or_incomplete_selection", []string{"-test.run=^TestBackendFacts$/^absent$"}},
		{"skip-all", "1", privateMarker, "skipped_selection_forbidden", []string{"-test.skip=TestBackend"}},
		{"skip-owner", "1", privateMarker, "skipped_selection_forbidden", []string{"-test.skip=^TestBackendSearchFacts$/x$"}},
		{"count-not-explicit", "1", privateMarker, "uncached_execution_required", nil},
		{"repeat", "1", privateMarker, "repeated_execution_forbidden", []string{"-test.count=2"}},
		{"zero-count", "1", privateMarker, "repeated_execution_forbidden", []string{"-test.count=0"}},
		{"cpu-repeat", "1", privateMarker, "repeated_execution_forbidden", []string{"-test.cpu=1,2"}},
		{"zero-timeout", "1", privateMarker, "invalid_scenario_timeout", []string{"-grok-facts-timeout=0"}},
		{"unbounded-timeout", "1", privateMarker, "invalid_scenario_timeout", []string{"-grok-facts-timeout=4m"}},
		{"private-endpoint", "1", privateMarker, "invalid_endpoint", []string{"-grok-facts-endpoint=https://user:" + privateMarker + "@example.invalid/responses"}},
		{"query-endpoint", "1", privateMarker, "invalid_endpoint", []string{"-grok-facts-endpoint=" + server.URL + "?key=" + privateMarker}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			args := []string{"-grok-facts-endpoint=" + server.URL}
			if tc.name != "count-not-explicit" {
				args = append(args, "-test.count=1")
			}
			output, code := tagged(t, append(args, tc.args...), tc.optIn, tc.key)
			if code == 0 || !strings.Contains(output, `"reason":"`+tc.reason+`"`) || !strings.Contains(output, `"selected":0,"completed":0`) || requests.Load() != 0 {
				t.Fatalf("unproven selection changed state or became success: %s", output)
			}
		})
	}
	output, code := tagged(t, []string{"-test.list=TestBackend"}, "0", "")
	if code != 0 || !strings.Contains(output, "TestBackendFacts") || !strings.Contains(output, "TestBackendSearchFacts") || strings.Contains(output, `"outcome":"observed"`) || requests.Load() != 0 {
		t.Fatal("credential-free discovery failed or claimed observation")
	}
}

func TestTaggedFactsExecuteEveryCatalogCaseWithBackendOnlyEvidence(t *testing.T) {
	var requests atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		if r.Method != http.MethodPost || r.Header.Get("Authorization") != "Bearer "+privateMarker {
			t.Error("tagged Facts changed the probe request boundary")
		}
		// Synthetic local traffic exercises dispatch and redaction, never provider
		// availability. Owning-package tests retain the exact replay/policy oracles.
		_, _ = w.Write([]byte(`{"status":"completed","model":"backend-alias","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"` + privateMarker + `"}]},{"type":"reasoning","encrypted_content":"` + privateMarker + `"},{"type":"web_search_call","id":"` + privateMarker + `","status":"completed","action":{"type":"search","query":"` + privateMarker + `"}},{"type":"custom_tool_call","id":"` + privateMarker + `","call_id":"` + privateMarker + `","name":"x_keyword_search","status":"completed","input":"` + privateMarker + `"}]}`))
	}))
	defer server.Close()
	for _, tc := range []struct {
		name  string
		args  []string
		cases int
		posts int32
	}{
		{"full", nil, 11, 15},
		{"smoke", []string{"-grok-facts-suite=smoke"}, 4, 5},
		{"one-backend-x", []string{"-grok-facts-scenario=x"}, 1, 1},
		{"explicit-diagnostic", []string{"-grok-facts-suite=diagnostic", "-grok-facts-scenario=x-window"}, 1, 1},
		{"go-subtest", []string{"-test.run=^TestBackendFacts$/^encrypted-replay$"}, 1, 2},
	} {
		t.Run(tc.name, func(t *testing.T) {
			before := requests.Load()
			args := []string{"-test.count=1", "-test.v", "-grok-facts-endpoint=" + server.URL}
			output, code := tagged(t, append(args, tc.args...), "1", privateMarker)
			if code != 0 || requests.Load()-before != tc.posts || !strings.Contains(output, `"scope":"direct_backend","outcome":"observed"`) || strings.Contains(output, server.URL) {
				t.Fatalf("tagged selection lost bounded backend-only evidence: %s", output)
			}
			var summary struct{ Selected, Completed int }
			lines := strings.Split(strings.TrimSpace(output), "\n")
			if json.Unmarshal([]byte(lines[len(lines)-1]), &summary) != nil || summary.Selected != tc.cases || summary.Completed != tc.cases {
				t.Fatalf("selected Facts inventory changed: %s", output)
			}
		})
	}
}

func TestTaggedFactsFailuresNeverCompleteOrRetry(t *testing.T) {
	for _, scenario := range []string{"basic-primary", "encrypted-replay", "web-replay", "x-replay"} {
		t.Run(scenario, func(t *testing.T) {
			var requests atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				requests.Add(1)
				_, _ = w.Write([]byte(`{"status":"completed","output":[]}`))
			}))
			defer server.Close()
			output, code := tagged(t, []string{"-test.count=1", "-grok-facts-scenario=" + scenario, "-grok-facts-endpoint=" + server.URL}, "1", privateMarker)
			if code == 0 || requests.Load() != 1 || !strings.Contains(output, `"outcome":"not_observed"`) || !strings.Contains(output, `"selected":1,"completed":0`) {
				t.Fatalf("incomplete backend result was credited or retried: %s", output)
			}
		})
	}
}

func TestTaggedFactsDeadlineStopsOneRequest(t *testing.T) {
	var requests atomic.Int32
	release := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		select {
		case <-r.Context().Done():
		case <-release:
		}
	}))
	defer server.Close()
	defer close(release)
	output, code := tagged(t, []string{"-test.count=1", "-grok-facts-scenario=encrypted-replay", "-grok-facts-timeout=100ms", "-grok-facts-endpoint=" + server.URL}, "1", privateMarker)
	if code == 0 || requests.Load() != 1 || !strings.Contains(output, `"selected":1,"completed":0`) {
		t.Fatalf("bounded deadline allowed continuation or success: %s", output)
	}
}
