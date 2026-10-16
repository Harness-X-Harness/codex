package main

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func tagged(t *testing.T, args []string, optIn, key, binary string) (string, int) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	command := exec.CommandContext(ctx, taggedPath, args...)
	for _, value := range os.Environ() {
		if !strings.HasPrefix(value, "GROK_LIVE=") && !strings.HasPrefix(value, "GROK_API_KEY=") && !strings.HasPrefix(value, "GROK_CODEX_BIN=") && !strings.HasPrefix(value, "GROK_LIVE_NATIVE_BIN=") {
			command.Env = append(command.Env, value)
		}
	}
	command.Env = append(command.Env, "GROK_LIVE="+optIn, "GROK_API_KEY="+key, "GROK_CODEX_BIN="+binary)
	var stdout, stderr bytes.Buffer
	command.Stdout, command.Stderr = &stdout, &stderr
	err := command.Run()
	code := 0
	if err != nil {
		exit, ok := err.(*exec.ExitError)
		if !ok || ctx.Err() != nil {
			t.Fatalf("tagged command failed outside result contract: %v", err)
		}
		code = exit.ExitCode()
	}
	if (stderr.Len() != 0 && stderr.String() != "testing: warning: no tests to run\n") || strings.Contains(stdout.String()+stderr.String(), "PRIVATE_") || stdout.Len() > 1<<20 {
		t.Fatal("tagged entrypoint leaked input or unbounded output")
	}
	return stdout.String(), code
}

func TestTaggedLiveRejectsUnprovenSelectionsWithoutLaunchingRuntime(t *testing.T) {
	trace := filepath.Join(t.TempDir(), "PRIVATE_TRACE")
	key, _ := json.Marshal(map[string]string{"Mode": "turn", "Trace": trace})
	for _, tc := range []struct {
		name                       string
		args                       []string
		optIn, key, binary, reason string
	}{
		{"missing-opt-in", nil, "0", string(key), fixturePath, "explicit_opt_in_required"},
		{"missing-key", nil, "1", "", fixturePath, "credential_required"},
		{"missing-binary", nil, "1", string(key), "", "binary_selection_invalid"},
		{"unknown-scenario", []string{"-grok-live-scenario=PRIVATE_UNKNOWN"}, "1", string(key), fixturePath, "invalid_scenario_selection"},
		{"unknown-suite", []string{"-grok-live-suite=PRIVATE_UNKNOWN"}, "1", string(key), fixturePath, "invalid_scenario_selection"},
		{"diagnostic-requires-choice", []string{"-grok-live-suite=diagnostic"}, "1", string(key), fixturePath, "diagnostic_scenario_required"},
		{"unmatched-run", []string{"-test.run=^Absent$"}, "1", string(key), fixturePath, "empty_or_incomplete_selection"},
		{"unmatched-subtest", []string{"-test.run=^TestLive$/^absent$"}, "1", string(key), fixturePath, "empty_or_incomplete_selection"},
		{"skip-all", []string{"-test.skip=TestLive"}, "1", string(key), fixturePath, "skipped_selection_forbidden"},
		{"skip-smoke-owner", []string{"-test.skip=^TestLive$/web$"}, "1", string(key), fixturePath, "skipped_selection_forbidden"},
		{"skip-full-owner", []string{"-grok-live-suite=full", "-test.skip=^TestLive$/web$"}, "1", string(key), fixturePath, "skipped_selection_forbidden"},
		{"skip-private-input", []string{"-test.skip=PRIVATE_PATTERN"}, "1", string(key), fixturePath, "skipped_selection_forbidden"},
		{"count-not-explicit", nil, "1", string(key), fixturePath, "uncached_execution_required"},
		{"repeat", []string{"-test.count=2"}, "1", string(key), fixturePath, "repeated_execution_forbidden"},
		{"cpu-repeat", []string{"-test.cpu=1,2"}, "1", string(key), fixturePath, "repeated_execution_forbidden"},
		{"metadata-without-verification", []string{"-grok-live-source-sha=PRIVATE_REVISION"}, "1", string(key), fixturePath, "verified_metadata_requires_explicit_subject"},
		{"bad-verified-subject", []string{"-grok-live-verified-subject", "-grok-live-binary-sha256=PRIVATE_DIGEST"}, "1", string(key), fixturePath, "verified_subject_invalid"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			args := tc.args
			if tc.name != "count-not-explicit" {
				args = append([]string{"-test.count=1"}, args...)
			}
			output, code := tagged(t, args, tc.optIn, tc.key, tc.binary)
			if code == 0 || !strings.Contains(output, `"reason":"`+tc.reason+`"`) || !strings.Contains(output, `"selected":0,"completed":0`) {
				t.Fatalf("required selection became success: %s", output)
			}
			if _, err := os.Stat(trace); !os.IsNotExist(err) {
				t.Fatal("guard launched the runtime")
			}
		})
	}
	output, code := tagged(t, []string{"-test.list=TestLive"}, "0", "", "")
	if code != 0 || !strings.Contains(output, "TestLive") {
		t.Fatal("credential-free discovery failed")
	}
}

func TestTaggedLiveRunsOneSelectedCaseWithHonestLocalSubject(t *testing.T) {
	trace := filepath.Join(t.TempDir(), "PRIVATE_TRACE")
	key, _ := json.Marshal(map[string]string{"Mode": "turn", "Trace": trace})
	output, code := tagged(t, []string{"-test.count=1", "-test.v", "-test.run=^TestLive$/^basic-primary$"}, "1", string(key), fixturePath)
	if code != 0 || !strings.Contains(output, `"selected":1,"completed":1`) || !strings.Contains(output, `"outcome":"observed"`) || !strings.Contains(output, `"SourceSHA":"","HarnessSHA":""`) || !strings.Contains(output, `"LocalUnverified":true`) {
		t.Fatalf("local tagged evidence: %s", output)
	}
	transcript, err := os.ReadFile(trace)
	if err != nil || strings.Count(string(transcript), "initialize\n") != 1 || strings.Count(string(transcript), "turn/start\n") != 1 {
		t.Fatal("tagged selection repeated semantic work")
	}
}

func TestTaggedLiveFailedCaseCannotCountAsCompleted(t *testing.T) {
	trace := filepath.Join(t.TempDir(), "PRIVATE_TRACE")
	key, _ := json.Marshal(map[string]string{"Mode": "failed_typed", "Trace": trace})
	output, code := tagged(t, []string{"-test.count=1", "-test.v", "-grok-live-scenario=basic-primary"}, "1", string(key), fixturePath)
	if code == 0 || !strings.Contains(output, `"selected":1,"completed":0`) || !strings.Contains(output, `"outcome":"not_proven"`) || !strings.Contains(output, `"code":"turn_unsuccessful"`) {
		t.Fatalf("failed case became completed: %s", output)
	}
	transcript, err := os.ReadFile(trace)
	if err != nil || strings.Count(string(transcript), "initialize\n") != 1 || strings.Count(string(transcript), "turn/start\n") != 1 {
		t.Fatal("failed tagged case resubmitted semantic work")
	}
}

func TestTaggedLivePreservesCallerVerifiedSubject(t *testing.T) {
	key, _ := json.Marshal(map[string]string{"Mode": "turn", "Trace": filepath.Join(t.TempDir(), "PRIVATE_TRACE")})
	args := []string{"-test.count=1", "-test.v", "-grok-live-scenario=basic-primary", "-grok-live-verified-subject", "-grok-live-binary-sha256=" + fixtureDigest, "-grok-live-source-sha=" + strings.Repeat("a", 40), "-grok-live-harness-sha=" + strings.Repeat("b", 40), "-grok-live-target=x86_64-unknown-linux-musl", "-grok-live-environment=deterministic"}
	output, code := tagged(t, args, "1", string(key), fixturePath)
	if code != 0 || strings.Contains(output, `"LocalUnverified":true`) || !strings.Contains(output, `"SourceSHA":"`+strings.Repeat("a", 40)+`","HarnessSHA":"`+strings.Repeat("b", 40)+`"`) {
		t.Fatalf("verified subject changed: %s", output)
	}
}

func TestTaggedLiveReportsExpectedXRejectionWithoutSearchCompletion(t *testing.T) {
	trace := filepath.Join(t.TempDir(), "PRIVATE_TRACE")
	key, _ := json.Marshal(map[string]string{"Mode": "search:public_only:x", "Trace": trace})
	output, code := tagged(t, []string{"-test.count=1", "-test.v", "-grok-live-suite=full", "-grok-live-scenario=x"}, "1", string(key), fixturePath)
	if code != 0 || !strings.Contains(output, `"outcome":"expected_unsupported"`) || !strings.Contains(output, `"outcome":"accepted"`) || !strings.Contains(output, `"selected":1,"completed":1,"expected_unsupported":1`) || !strings.Contains(output, `"Completed":false`) || strings.Contains(output, `"outcome":"observed"`) {
		t.Fatalf("tagged rejection misreported: %s", output)
	}
	transcript, err := os.ReadFile(trace)
	if err != nil || strings.Count(string(transcript), "turn/start\n") != 1 {
		t.Fatal("tagged rejection repeated work")
	}
}
