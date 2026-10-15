package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/dist"
	"github.com/Harness-X-Harness/codex/grok/facts"
	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
)

var commandPath string

const sourceSHA = "0123456789abcdef0123456789abcdef01234567"
const privateMarker = "PRIVATE_CLI_CANARY"

func TestMain(m *testing.M) {
	dir, err := os.MkdirTemp("", "grok-facts-command-")
	if err != nil {
		fmt.Fprintln(os.Stderr, "cannot create command fixture")
		os.Exit(1)
	}
	name := "grok-facts"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	commandPath = filepath.Join(dir, name)
	build := exec.Command("go", "build", "-race", "-o", commandPath, ".")
	if output, err := build.CombinedOutput(); err != nil {
		fmt.Fprintf(os.Stderr, "cannot build actual command: %v\n%s", err, output)
		_ = os.RemoveAll(dir)
		os.Exit(1)
	}
	code := m.Run()
	_ = os.RemoveAll(dir)
	os.Exit(code)
}

func invoke(t *testing.T, args []string, optIn, key string) (report, int) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, commandPath, args...)
	for _, value := range os.Environ() {
		if !strings.HasPrefix(value, "GROK_FACTS=") && !strings.HasPrefix(value, "GROK_API_KEY=") {
			cmd.Env = append(cmd.Env, value)
		}
	}
	cmd.Env = append(cmd.Env, "GROK_FACTS="+optIn, "GROK_API_KEY="+key)
	var stdout, stderr bytes.Buffer
	cmd.Stdout, cmd.Stderr = &stdout, &stderr
	err := cmd.Run()
	code := 0
	if err != nil {
		exit, ok := err.(*exec.ExitError)
		if !ok || ctx.Err() != nil {
			t.Fatalf("command failed outside its result contract: %v", err)
		}
		code = exit.ExitCode()
	}
	if stderr.Len() != 0 || strings.Contains(stdout.String(), privateMarker) {
		t.Fatal("command leaked private input or wrote unexpected stderr")
	}
	var result report
	decoder := json.NewDecoder(&stdout)
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&result); err != nil {
		t.Fatalf("missing structured result: %v", err)
	}
	var extra any
	if decoder.Decode(&extra) != io.EOF {
		t.Fatal("command emitted more than one result")
	}
	if _, err := time.Parse(time.RFC3339Nano, result.RecordedAt); err != nil || !strings.HasSuffix(result.RecordedAt, "Z") {
		t.Fatal("result lacks a valid UTC observation timestamp")
	}
	return result, code
}

func arguments(scenario, endpoint string) []string {
	return []string{"--scenario", scenario, "--source-sha", sourceSHA, "--endpoint", endpoint}
}

func TestCommandInvokesEachExistingScenario(t *testing.T) {
	models, err := dist.ModelSlugs()
	if err != nil {
		t.Fatal(err)
	}
	cases := []struct {
		name   string
		models []string
		tool   string
	}{
		{"basic-primary", []string{providerfixture.PrimaryModel}, ""},
		{"basic-pinned", []string{providerfixture.PinnedModel}, ""},
		{"encrypted-replay", []string{providerfixture.PrimaryModel, providerfixture.PrimaryModel}, ""},
		{"shipped-routes", models, ""},
		{"web", []string{providerfixture.PrimaryModel}, `{"type":"web_search"}`},
		{"web-allowed", []string{providerfixture.PrimaryModel}, `{"type":"web_search","filters":{"allowed_domains":["reuters.com"]}}`},
		{"web-excluded", []string{providerfixture.PrimaryModel}, `{"type":"web_search","filters":{"excluded_domains":["example.com"]}}`},
		{"x", []string{providerfixture.PrimaryModel}, `{"type":"x_search"}`},
		{"x-window", []string{providerfixture.PrimaryModel}, `{"type":"x_search","from_date":"2026-08-01","to_date":"2026-08-15"}`},
		{"web-replay", []string{providerfixture.PrimaryModel, providerfixture.PrimaryModel}, `{"type":"web_search"}`},
		{"x-replay", []string{providerfixture.PrimaryModel, providerfixture.PrimaryModel}, `{"type":"x_search"}`},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			var mu sync.Mutex
			var requests []map[string]any
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				var body map[string]any
				if r.Method != http.MethodPost || r.Header.Get("Authorization") != "Bearer "+privateMarker || json.NewDecoder(r.Body).Decode(&body) != nil {
					t.Error("command changed the HTTP request boundary")
				}
				mu.Lock()
				requests = append(requests, body)
				n := len(requests)
				mu.Unlock()
				output := []any{map[string]any{"type": "message", "role": "assistant", "content": []any{map[string]any{"type": "output_text", "text": privateMarker}}}}
				if n == 1 {
					switch tc.name {
					case "encrypted-replay":
						output = append(output, map[string]any{"type": "reasoning", "encrypted_content": privateMarker})
					case "web-replay":
						output = append(output, map[string]any{"type": "web_search_call", "id": privateMarker, "status": "completed", "action": map[string]any{"type": "search", "query": privateMarker}})
					case "x-replay":
						output = append(output, map[string]any{"type": "custom_tool_call", "id": privateMarker, "call_id": privateMarker, "name": "x_keyword_search", "status": "completed", "input": privateMarker})
					}
				}
				_ = json.NewEncoder(w).Encode(map[string]any{"status": "completed", "model": "backend-alias", "output": output})
			}))
			defer server.Close()
			got, code := invoke(t, arguments(tc.name, server.URL), "1", privateMarker)
			digest := sha256.Sum256([]byte(server.URL))
			if got.EndpointSHA256 != hex.EncodeToString(digest[:]) {
				t.Fatal("result does not identify the selected endpoint")
			}
			if code != 0 || got.Outcome != "observed" || got.Reason != "" || got.Scenario != tc.name || got.DeclaredSourceSHA != sourceSHA || got.SchemaVersion != 1 {
				t.Fatalf("incorrect command result: %+v, exit=%d", got, code)
			}
			mu.Lock()
			defer mu.Unlock()
			if len(requests) != len(tc.models) {
				t.Fatalf("selected scenario made %d requests, want %d", len(requests), len(tc.models))
			}
			for i, body := range requests {
				if body["model"] != tc.models[i] || body["stream"] != false {
					t.Error("selected scenario did not retain its request model/transport")
				}
				var tool any
				if tc.tool != "" {
					_ = json.Unmarshal([]byte(tc.tool), &tool)
					if !reflect.DeepEqual(body["tools"], []any{tool}) {
						t.Error("scenario selected a different policy fixture")
					}
				} else if body["tools"] != nil {
					t.Error("text control acquired search tools")
				}
			}
			want := facts.Observation{Requests: len(tc.models), HTTPStatus: 200, Completed: true, TextBytes: len(privateMarker)}
			switch tc.name {
			case "basic-primary", "basic-pinned":
				want.Stage = "text"
				if got.Observation == nil || *got.Observation != want || got.Search != nil || got.Routes != nil {
					t.Fatal("text evidence changed")
				}
			case "encrypted-replay":
				want.Stage, want.EncryptedItems, want.Replayed = "replay", 1, true
				if got.Observation == nil || *got.Observation != want || got.Search != nil || got.Routes != nil {
					t.Fatal("replay evidence changed")
				}
			case "shipped-routes":
				var routes []facts.RouteObservation
				want.Stage, want.Requests = "text", 1
				for _, model := range models {
					routes = append(routes, facts.RouteObservation{Model: model, Observation: want})
				}
				if !reflect.DeepEqual(got.Routes, routes) || got.Observation != nil || got.Search != nil {
					t.Fatal("shipped route evidence changed")
				}
			default:
				want.Stage = "search_policy"
				scenario := strings.ReplaceAll(tc.name, "-", "_")
				hosted := 0
				if strings.HasSuffix(tc.name, "-replay") {
					want.Stage, want.Replayed, hosted = "search_replay", true, 1
					scenario = strings.TrimSuffix(tc.name, "-replay")
				}
				search := facts.SearchObservation{Observation: want, Scenario: scenario, HostedCalls: hosted}
				if got.Search == nil || *got.Search != search || got.Observation != nil || got.Routes != nil {
					t.Fatal("search evidence changed")
				}
			}
		})
	}
}

func TestCommandPreflightMakesNoBackendRequest(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) { calls.Add(1) }))
	defer server.Close()
	base := arguments("basic-primary", server.URL)
	cases := []struct {
		name                string
		args                []string
		optIn, key, outcome string
		exit                int
	}{
		{"not_opted_in", base, "0", privateMarker, "not_run", 3},
		{"missing_key", base, "1", "", "configuration_error", 2},
		{"missing_scenario", []string{"--source-sha", sourceSHA}, "1", privateMarker, "configuration_error", 2},
		{"bad_scenario", arguments(privateMarker, server.URL), "1", privateMarker, "configuration_error", 2},
		{"bad_source", []string{"--scenario", "basic-primary", "--source-sha", privateMarker}, "1", privateMarker, "configuration_error", 2},
		{"unknown_flag", []string{"--" + privateMarker}, "1", privateMarker, "configuration_error", 2},
		{"extra_argument", append(append([]string{}, base...), privateMarker), "1", privateMarker, "configuration_error", 2},
		{"long_timeout", append(append([]string{}, base...), "--timeout", "4m"), "1", privateMarker, "configuration_error", 2},
		{"invalid_timeout", append(append([]string{}, base...), "--timeout", privateMarker), "1", privateMarker, "configuration_error", 2},
		{"endpoint_userinfo", arguments("basic-primary", "https://user:"+privateMarker+"@example.invalid/responses"), "1", privateMarker, "configuration_error", 2},
		{"endpoint_query", arguments("basic-primary", server.URL+"?key="+privateMarker), "1", privateMarker, "configuration_error", 2},
		{"cleartext_remote", arguments("basic-primary", "http://example.invalid/responses"), "1", privateMarker, "configuration_error", 2},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got, code := invoke(t, tc.args, tc.optIn, tc.key)
			if code != tc.exit || got.Outcome != tc.outcome || got.Reason == "" || got.Observation != nil || got.Search != nil || got.Routes != nil {
				t.Fatalf("preflight result: %+v, %d", got, code)
			}
		})
	}
	if calls.Load() != 0 {
		t.Fatal("preflight reached backend")
	}
}

func TestCommandReportsFailureWithoutContinuationOrRawError(t *testing.T) {
	for _, status := range []int{200, 401, 403, 429, 500} {
		t.Run(fmt.Sprint(status), func(t *testing.T) {
			t.Parallel()
			var calls atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls.Add(1)
				w.WriteHeader(status)
				_, _ = io.WriteString(w, `{"status":"incomplete","error":{"message":"`+privateMarker+`"}}`)
			}))
			defer server.Close()
			got, code := invoke(t, arguments("web-replay", server.URL), "1", privateMarker)
			if code != 1 || got.Outcome != "not_observed" || got.Search == nil || got.Search.Completed || got.Search.Replayed || got.Search.Requests != 1 || got.Search.HTTPStatus != status || calls.Load() != 1 {
				t.Fatalf("failed observation accepted or resubmitted: %+v, %d", got, code)
			}
			want := "http_rejected"
			if status == 200 {
				want = "probe_did_not_complete"
			}
			if status == 401 || status == 403 {
				want = "authentication_or_access_rejected"
			}
			if got.Reason != want {
				t.Fatalf("reason=%s, want %s", got.Reason, want)
			}
		})
	}
}

func TestCommandDeadlineAndRedirectRemainNonSuccess(t *testing.T) {
	t.Run("deadline", func(t *testing.T) {
		release := make(chan struct{})
		server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			// Reading the request body lets net/http observe the client disconnect.
			if _, err := io.Copy(io.Discard, r.Body); err != nil {
				return
			}
			select {
			case <-r.Context().Done():
			case <-release:
			}
		}))
		defer server.Close()
		// A failed assertion must also release the fixture before Server.Close.
		defer close(release)
		args := append(arguments("encrypted-replay", server.URL), "--timeout", "50ms")
		got, code := invoke(t, args, "1", privateMarker)
		if code != 1 || got.Outcome != "not_observed" || got.Reason != "deadline_exceeded" || got.Observation == nil || got.Observation.Replayed {
			t.Fatal("deadline became an observation or replay")
		}
	})
	t.Run("redirect", func(t *testing.T) {
		var redirected atomic.Int32
		destination := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) { redirected.Add(1) }))
		defer destination.Close()
		server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			http.Redirect(w, r, destination.URL, http.StatusTemporaryRedirect)
		}))
		defer server.Close()
		got, code := invoke(t, arguments("basic-primary", server.URL), "1", privateMarker)
		if code != 1 || got.Outcome != "not_observed" || redirected.Load() != 0 {
			t.Fatal("redirect escaped selected boundary")
		}
	})
}

type failedOutput struct{}

func (failedOutput) Write([]byte) (int, error) { return 0, io.ErrClosedPipe }

func TestCommandCancellationAndUnwritableEvidence(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		_, _ = io.WriteString(w, `{"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}]}`)
	}))
	defer server.Close()
	getenv := func(name string) string {
		if name == "GROK_FACTS" {
			return "1"
		}
		if name == "GROK_API_KEY" {
			return privateMarker
		}
		return ""
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	var output bytes.Buffer
	code := run(ctx, arguments("basic-primary", server.URL), getenv, &output)
	var got report
	if json.Unmarshal(output.Bytes(), &got) != nil || code != 1 || got.Outcome != "not_observed" || got.Reason != "cancelled" || calls.Load() != 0 {
		t.Fatal("cancelled invocation reached backend or became successful")
	}
	if run(context.Background(), arguments("basic-primary", server.URL), getenv, failedOutput{}) != 1 || calls.Load() != 1 {
		t.Fatal("unwritable evidence returned success or resubmitted")
	}
}
