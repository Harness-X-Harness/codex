package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/live"
)

var commandPath, fixturePath, fixtureDigest string

func TestMain(m *testing.M) {
	dir, err := os.MkdirTemp("", "grok-live-command-")
	if err != nil {
		fmt.Fprintln(os.Stderr, "cannot create command fixture")
		os.Exit(1)
	}
	suffix := ""
	if runtime.GOOS == "windows" {
		suffix = ".exe"
	}
	commandPath, fixturePath = filepath.Join(dir, "grok-live"+suffix), filepath.Join(dir, "live-fixture"+suffix)
	for _, args := range [][]string{{"build", "-race", "-o", commandPath, "."}, {"test", "-c", "-race", "-o", fixturePath, "../../live"}} {
		if output, err := exec.Command("go", args...).CombinedOutput(); err != nil {
			fmt.Fprintf(os.Stderr, "cannot build actual command/owned fixture: %v\n%s", err, output)
			_ = os.RemoveAll(dir)
			os.Exit(1)
		}
	}
	file, err := os.Open(fixturePath)
	if err != nil {
		_ = os.RemoveAll(dir)
		os.Exit(1)
	}
	hash := sha256.New()
	_, err = io.Copy(hash, file)
	_ = file.Close()
	if err != nil {
		_ = os.RemoveAll(dir)
		os.Exit(1)
	}
	fixtureDigest = hex.EncodeToString(hash.Sum(nil))
	code := m.Run()
	_ = os.RemoveAll(dir)
	os.Exit(code)
}

func arguments(scenario string) []string {
	return []string{"--scenario", scenario, "--binary", fixturePath, "--binary-sha256", fixtureDigest,
		"--source-sha", strings.Repeat("a", 40), "--harness-sha", strings.Repeat("b", 40),
		"--target", "x86_64-unknown-linux-musl", "--environment", "deterministic", "--timeout", "20s"}
}

type wireReport struct {
	SchemaVersion  int             `json:"schema_version"`
	Outcome        string          `json:"outcome"`
	Reason         string          `json:"reason,omitempty"`
	Scenario       string          `json:"scenario,omitempty"`
	EndpointSHA256 string          `json:"endpoint_sha256,omitempty"`
	RecordedAt     string          `json:"recorded_at"`
	Evidence       json.RawMessage `json:"evidence,omitempty"`
}

func invoke(t *testing.T, args []string, optIn, key string) (wireReport, int) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	command := exec.CommandContext(ctx, commandPath, args...)
	for _, value := range os.Environ() {
		if !strings.HasPrefix(value, "GROK_LIVE=") && !strings.HasPrefix(value, "GROK_API_KEY=") {
			command.Env = append(command.Env, value)
		}
	}
	command.Env = append(command.Env, "GROK_LIVE="+optIn, "GROK_API_KEY="+key)
	var stdout, stderr bytes.Buffer
	command.Stdout, command.Stderr = &stdout, &stderr
	err := command.Run()
	code := 0
	if err != nil {
		exit, ok := err.(*exec.ExitError)
		if !ok || ctx.Err() != nil {
			t.Fatalf("command failed outside its result contract: %v", err)
		}
		code = exit.ExitCode()
	}
	if stderr.Len() != 0 || strings.Contains(stdout.String(), "PRIVATE_") {
		t.Fatal("command leaked private input/traffic or unexpected stderr")
	}
	var result wireReport
	decoder := json.NewDecoder(&stdout)
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&result); err != nil {
		t.Fatalf("missing structured result: %v", err)
	}
	var extra any
	if decoder.Decode(&extra) != io.EOF {
		t.Fatal("command emitted multiple results")
	}
	if _, err := time.Parse(time.RFC3339Nano, result.RecordedAt); err != nil || !strings.HasSuffix(result.RecordedAt, "Z") {
		t.Fatal("missing UTC recording timestamp")
	}
	return result, code
}

func TestCommandExecutesEveryOwnedScenario(t *testing.T) {
	cases := []struct {
		scenario, mode, model string
		turns                 int
	}{
		{"basic-primary", "turn", "grok-4.7", 1},
		{"basic-pinned", "pinned", "grok-4.6", 1},
		{"reasoning-primary", "history:turn", "grok-4.7", 2},
		{"reasoning-pinned", "history:turn", "grok-4.6", 2},
		{"structured-edit", "edit:success", "grok-4.7", 2},
		{"structured-edit-declined", "edit:decline", "grok-4.7", 1},
		{"structured-edit-pinned", "edit:pinned", "grok-4.6", 1},
		{"structured-edit-all", "edit:replace_all", "grok-4.7", 1},
		{"image-primary", "image:success", "grok-4.7", 2},
		{"image-pinned", "image:pinned", "grok-4.6", 2},
		{"shipped-catalog", "shipped:catalog", "grok-4.7", 0},
		{"shipped-startup", "shipped:startup", "grok-4.7", 1},
		{"shipped-pinned", "history:shipped", "grok-4.6", 2},
		{"shipped-child", "shipped:ok", "grok-4.7", 2},
		{"web", "search:ok:web", "grok-4.7", 2},
		{"web-allowed", "search:ok:web_allowed", "grok-4.7", 2},
		{"web-excluded", "search:ok:web_excluded", "grok-4.7", 2},
		{"x", "search:ok:x", "grok-4.7", 2},
		{"x-window", "search:ok:x_window", "grok-4.7", 2},
	}
	for _, tc := range cases {
		t.Run(tc.scenario, func(t *testing.T) {
			t.Parallel()
			trace := filepath.Join(t.TempDir(), "trace")
			key, _ := json.Marshal(map[string]string{"Mode": tc.mode, "Trace": trace})
			got, code := invoke(t, arguments(tc.scenario), "1", string(key))
			if code != 0 || got.SchemaVersion != 1 || got.Outcome != "observed" || got.Reason != "" || got.Scenario != tc.scenario || len(got.EndpointSHA256) != 64 {
				t.Fatalf("scenario result: %+v exit=%d", got, code)
			}
			var evidence live.Evidence
			if json.Unmarshal(got.Evidence, &evidence) != nil || evidence.SHA256 != fixtureDigest || evidence.SourceSHA != strings.Repeat("a", 40) || evidence.HarnessSHA != strings.Repeat("b", 40) || evidence.Model != tc.model || evidence.Target != "x86_64-unknown-linux-musl" || evidence.Environment != "deterministic" || evidence.Processes != 1 || evidence.Initializations != 1 || evidence.Turns != tc.turns {
				t.Fatalf("owned evidence or invocation boundary changed: %+v", evidence)
			}
			if tc.scenario == "shipped-catalog" {
				if !evidence.ShippedCatalog || evidence.Completed || evidence.CatalogModels != 2 {
					t.Fatal("catalog-only result became inference evidence")
				}
			} else if !evidence.Completed {
				t.Fatal("incomplete owned scenario became successful")
			}
			transcript, err := os.ReadFile(trace)
			if err != nil || strings.Count(string(transcript), "initialize\n") != 1 || strings.Count(string(transcript), "turn/start\n") != tc.turns {
				t.Fatal("command duplicated initialization or semantic turn submission")
			}
		})
	}
}

func TestCommandRejectsPreflightWithoutStartingRuntime(t *testing.T) {
	trace := filepath.Join(t.TempDir(), "trace")
	key, _ := json.Marshal(map[string]string{"Mode": "turn", "Trace": trace})
	cases := []struct {
		name, optIn, key, outcome string
		extra                     []string
		code                      int
	}{
		{"not_opted_in", "0", string(key), "not_run", nil, 3},
		{"missing_key", "1", "", "configuration_error", nil, 2},
		{"bad_scenario", "1", string(key), "configuration_error", []string{"--scenario", "PRIVATE_CANARY"}, 2},
		{"bad_digest", "1", string(key), "configuration_error", []string{"--binary-sha256", strings.Repeat("0", 64)}, 2},
		{"bad_source", "1", string(key), "configuration_error", []string{"--source-sha", "PRIVATE_CANARY"}, 2},
		{"missing_binary", "1", string(key), "configuration_error", []string{"--binary", filepath.Join(t.TempDir(), "absent")}, 2},
		{"wrong_target", "1", string(key), "configuration_error", []string{"--target", "other"}, 2},
		{"deadline", "1", string(key), "configuration_error", []string{"--timeout", "11m"}, 2},
		{"endpoint_secret", "1", string(key), "configuration_error", []string{"--endpoint", "https://example.invalid?key=PRIVATE_CANARY"}, 2},
		{"shipped_override", "1", string(key), "configuration_error", []string{"--scenario", "shipped-startup", "--endpoint", "https://example.invalid"}, 2},
		{"unknown_flag", "1", string(key), "configuration_error", []string{"--PRIVATE_CANARY"}, 2},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got, code := invoke(t, append(arguments("basic-primary"), tc.extra...), tc.optIn, tc.key)
			if code != tc.code || got.Outcome != tc.outcome || got.Reason == "" {
				t.Fatalf("preflight: %+v exit=%d", got, code)
			}
			if _, err := os.Stat(trace); !os.IsNotExist(err) {
				t.Fatal("preflight launched the runtime")
			}
		})
	}
}

func TestCommandRetainsFailedScenarioWithoutResubmission(t *testing.T) {
	trace := filepath.Join(t.TempDir(), "trace")
	key, _ := json.Marshal(map[string]string{"Mode": "failed", "Trace": trace})
	got, code := invoke(t, arguments("basic-primary"), "1", string(key))
	var evidence live.Evidence
	if json.Unmarshal(got.Evidence, &evidence) != nil || code != 1 || got.Outcome != "not_observed" || evidence.Completed || evidence.Processes != 1 || evidence.Turns != 1 {
		t.Fatalf("failed scenario result: %+v exit=%d", got, code)
	}
	transcript, err := os.ReadFile(trace)
	if err != nil || strings.Count(string(transcript), "initialize\n") != 1 || strings.Count(string(transcript), "turn/start\n") != 1 {
		t.Fatal("failed scenario was restarted or resubmitted")
	}
}

func TestCommandDeadlineStopsAnIncompleteScenario(t *testing.T) {
	trace := filepath.Join(t.TempDir(), "trace")
	key, _ := json.Marshal(map[string]string{"Mode": "hang", "Trace": trace})
	args := append(arguments("basic-primary"), "--timeout", "2s")
	got, code := invoke(t, args, "1", string(key))
	var evidence live.Evidence
	if json.Unmarshal(got.Evidence, &evidence) != nil || code != 1 || got.Outcome != "not_observed" || got.Reason != "deadline_exceeded" || evidence.Completed || evidence.Processes != 1 || evidence.Turns != 1 {
		t.Fatalf("deadline result: %+v exit=%d", got, code)
	}
	transcript, err := os.ReadFile(trace)
	if err != nil || strings.Count(string(transcript), "initialize\n") != 1 || strings.Count(string(transcript), "turn/start\n") != 1 {
		t.Fatal("deadline resubmitted the incomplete scenario")
	}
}

func TestCommandInterruptRetainsCancelledResult(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("the supported package targets use Unix interrupt semantics")
	}
	trace := filepath.Join(t.TempDir(), "trace")
	key, _ := json.Marshal(map[string]string{"Mode": "hang", "Trace": trace})
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	command := exec.CommandContext(ctx, commandPath, arguments("basic-primary")...)
	for _, entry := range os.Environ() {
		if !strings.HasPrefix(entry, "GROK_LIVE=") && !strings.HasPrefix(entry, "GROK_API_KEY=") {
			command.Env = append(command.Env, entry)
		}
	}
	command.Env = append(command.Env, "GROK_LIVE=1", "GROK_API_KEY="+string(key))
	var stdout, stderr bytes.Buffer
	command.Stdout, command.Stderr = &stdout, &stderr
	if err := command.Start(); err != nil {
		t.Fatal(err)
	}
	for {
		transcript, _ := os.ReadFile(trace)
		if strings.Contains(string(transcript), "turn/start\n") {
			break
		}
		if ctx.Err() != nil {
			_ = command.Wait()
			t.Fatal("scenario did not reach the interrupt gate")
		}
		time.Sleep(10 * time.Millisecond)
	}
	if err := command.Process.Signal(os.Interrupt); err != nil {
		cancel()
		_ = command.Wait()
		t.Fatal(err)
	}
	err := command.Wait()
	exit, ok := err.(*exec.ExitError)
	var got wireReport
	if !ok || exit.ExitCode() != 1 || ctx.Err() != nil || stderr.Len() != 0 || json.Unmarshal(stdout.Bytes(), &got) != nil || got.Outcome != "not_observed" || got.Reason != "cancelled" {
		t.Fatalf("interrupt result: %+v; exit=%v", got, err)
	}
	transcript, err := os.ReadFile(trace)
	if err != nil || strings.Count(string(transcript), "initialize\n") != 1 || strings.Count(string(transcript), "turn/start\n") != 1 {
		t.Fatal("interrupt resubmitted the incomplete scenario")
	}
}

type failedWriter struct{}

func (failedWriter) Write([]byte) (int, error) { return 0, io.ErrClosedPipe }

func TestCommandHelpAndEvidenceWriteFailure(t *testing.T) {
	noEnvironment := func(string) string { return "" }
	var help bytes.Buffer
	if code := run(context.Background(), []string{"--help"}, noEnvironment, &help); code != 0 || help.String() != usage {
		t.Fatal("help required backend configuration")
	}
	if code := run(context.Background(), []string{"--help"}, noEnvironment, failedWriter{}); code != 1 {
		t.Fatal("unwritten help succeeded")
	}
	if code := run(context.Background(), arguments("basic-primary"), noEnvironment, failedWriter{}); code != 1 {
		t.Fatal("unwritten evidence was reported as a structured outcome")
	}
}
