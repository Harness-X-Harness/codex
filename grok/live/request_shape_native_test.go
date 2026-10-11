package live

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
)

// This is a controlled first-request diagnostic, not real-provider acceptance.
// A synthetic denial ends every case before tool execution or history replay.
func TestNativeFirstRequestShape(t *testing.T) {
	binary := os.Getenv("GROK_LIVE_NATIVE_BIN")
	if binary == "" {
		t.Skip("activated by the required native runtime job")
	}
	file, err := os.Open(binary)
	if err != nil {
		t.Fatal(err)
	}
	hash := sha256.New()
	_, copyErr := io.Copy(hash, file)
	closeErr := file.Close()
	if copyErr != nil || closeErr != nil {
		t.Fatal("native executable digest unavailable")
	}
	subject := Subject{Binary: binary, SHA256: hex.EncodeToString(hash.Sum(nil)),
		SourceSHA: os.Getenv("GITHUB_SHA"), HarnessSHA: os.Getenv("GITHUB_SHA"),
		Target: "x86_64-unknown-linux-gnu", Environment: "native-fixture"}
	if source := os.Getenv("GROK_LIVE_NATIVE_SOURCE_SHA"); source != "" {
		subject.SourceSHA = source
	}
	if target := os.Getenv("GROK_LIVE_NATIVE_TARGET"); target != "" {
		subject.Target, subject.Environment = target, "package-fixture"
	}
	for _, model := range []string{providerfixture.PinnedModel, providerfixture.PrimaryModel} {
		for _, profile := range []string{"isolated-history", "shipped-history", "isolated-edit"} {
			t.Run(model+"/"+profile, func(t *testing.T) {
				var mu sync.Mutex
				requests := 0
				var shape firstRequestShape
				valid := true
				backend := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
					defer r.Body.Close()
					mu.Lock()
					defer mu.Unlock()
					requests++
					data, err := io.ReadAll(io.LimitReader(r.Body, (1<<20)+1))
					if err != nil || len(data) > 1<<20 || requests != 1 ||
						r.Method != "POST" || r.URL.Path != "/v1/responses" ||
						r.Header.Get("Authorization") != "Bearer fixture-key" {
						valid = false
					} else if got, ok := summarizeFirstRequest(data); !ok {
						valid = false
					} else {
						shape = got
					}
					w.Header().Set("Content-Type", "application/json")
					w.WriteHeader(http.StatusForbidden)
					_, _ = io.WriteString(w, `{"error":{"message":"controlled fixture denial","type":"permission_error","code":"fixture_denied"}}`)
				}))
				defer backend.Close()
				options := Options{Subject: subject, Model: model, BaseURL: backend.URL + "/v1", APIKey: "fixture-key"}
				if profile == "shipped-history" {
					options.shippedProfile = true
					options.fixtureReady = func(home string) error {
						return substituteShippedEndpoint(home, options.BaseURL)
					}
				}
				ctx, cancel := context.WithTimeout(context.Background(), time.Minute)
				defer cancel()
				var evidence Evidence
				var runErr error
				if profile == "isolated-edit" {
					// The pinned public wrapper selects this same one-turn fixture.
					got, err := runStructuredEdit(ctx, options, editFixture{})
					evidence, runErr = got.Evidence, err
				} else {
					evidence, runErr = ReasoningHistory(ctx, options)
				}
				mu.Lock()
				defer mu.Unlock()
				failure := DescribeFailure(runErr)
				encoded, _ := json.Marshal(shape)
				// Never emit raw request/response bodies, prompts, headers or IDs.
				t.Logf("controlled first request: source=%s harness=%s target=%s executable_sha256=%s profile=%s requests=%d shape=%s failure=%+v",
					subject.SourceSHA, subject.HarnessSHA, subject.Target, subject.SHA256, profile, requests, encoded, failure)
				if runErr == nil || !valid || requests != 1 || !evidence.Bound || evidence.Completed || evidence.Turns != 1 ||
					failure.Code != "turn_unsuccessful" || failure.TerminalStatus != "failed" || failure.HTTPStatus != 403 {
					t.Fatal("controlled denial must remain unsuccessful after exactly one HTTP request")
				}
				if shape.Model != model || !shape.Stream || !containsShapeValue(shape.Include, "reasoning.encrypted_content") {
					t.Error("model, streaming or encrypted-reasoning request contract changed")
				}
				tool := historyTool
				if profile == "isolated-edit" {
					tool = "structured_edit"
				} else if shape.Effort != "medium" {
					t.Error("explicit history effort was not preserved")
				}
				if !containsShapeValue(shape.Tools, tool) {
					t.Error("scenario tool declaration missing")
				}
				// Other presence/defaults are observations, not guessed backend rules.
			})
		}
	}
}

type firstRequestShape struct {
	Model             string         `json:"model"`
	Stream            bool           `json:"stream"`
	Store             *bool          `json:"store_value,omitempty"`
	Fields            []string       `json:"fields"`
	Effort            string         `json:"effort,omitempty"`
	ReasoningFields   []string       `json:"reasoning_fields"`
	Include           []string       `json:"include"`
	InstructionsBytes int            `json:"instructions_bytes"`
	Tools             []string       `json:"tools"`
	InputTypes        map[string]int `json:"input_types"`
}

func summarizeFirstRequest(data []byte) (firstRequestShape, bool) {
	var body map[string]json.RawMessage
	shape := firstRequestShape{InputTypes: map[string]int{}}
	if json.Unmarshal(data, &body) != nil || len(body) > 32 {
		return shape, false
	}
	for key := range body {
		shape.Fields = append(shape.Fields, key)
	}
	sort.Strings(shape.Fields)
	var instructions string
	var reasoning map[string]json.RawMessage
	var tools []struct {
		Name string `json:"name"`
	}
	var input []struct {
		Type string `json:"type"`
	}
	if json.Unmarshal(body["model"], &shape.Model) != nil || json.Unmarshal(body["stream"], &shape.Stream) != nil ||
		json.Unmarshal(body["include"], &shape.Include) != nil || json.Unmarshal(body["input"], &input) != nil || len(input) > 128 {
		return shape, false
	}
	if raw, ok := body["instructions"]; ok && json.Unmarshal(raw, &instructions) != nil {
		return shape, false
	}
	shape.InstructionsBytes = len(instructions)
	if raw, ok := body["store"]; ok && json.Unmarshal(raw, &shape.Store) != nil {
		return shape, false
	}
	if raw, ok := body["reasoning"]; ok {
		if json.Unmarshal(raw, &reasoning) != nil {
			return shape, false
		}
		for key := range reasoning {
			shape.ReasoningFields = append(shape.ReasoningFields, key)
		}
		sort.Strings(shape.ReasoningFields)
		if raw, ok := reasoning["effort"]; ok && string(raw) != "null" && json.Unmarshal(raw, &shape.Effort) != nil {
			return shape, false
		}
	}
	if raw, ok := body["tools"]; ok {
		if json.Unmarshal(raw, &tools) != nil || len(tools) > 128 {
			return shape, false
		}
		for _, tool := range tools {
			shape.Tools = append(shape.Tools, tool.Name)
		}
		sort.Strings(shape.Tools)
	}
	for _, item := range input {
		shape.InputTypes[item.Type]++
	}
	return shape, true
}

func containsShapeValue(tools []string, want string) bool {
	for _, tool := range tools {
		if tool == want {
			return true
		}
	}
	return false
}

func TestFirstRequestShapeSummary(t *testing.T) {
	data := []byte(`{"model":"grok-4.6","stream":true,"include":["reasoning.encrypted_content"],"instructions":"PRIVATE_PROMPT","input":[{"type":"message","content":[{"text":"PRIVATE_INPUT"}]}],"tools":[{"name":"grok_history_probe","description":"PRIVATE_DESCRIPTION","parameters":{"secret":"PRIVATE_SCHEMA"}}],"reasoning":{"effort":"medium"},"prompt_cache_key":"PRIVATE_ID"}`)
	shape, ok := summarizeFirstRequest(data)
	encoded, err := json.Marshal(shape)
	if !ok || err != nil || strings.Contains(string(encoded), "PRIVATE_") ||
		shape.Model != "grok-4.6" || shape.Effort != "medium" || shape.InstructionsBytes != len("PRIVATE_PROMPT") ||
		shape.InputTypes["message"] != 1 || !containsShapeValue(shape.Tools, historyTool) {
		t.Fatal("shape summary must retain structural metadata without content")
	}
	// Missing optional fields and an explicit store value are observations, not
	// claims that a particular provider deployment accepts those choices.
	for _, optional := range []string{``, `,"store":false`, `,"store":true,"reasoning":null`} {
		shape, ok := summarizeFirstRequest([]byte(`{"model":"grok-4.7","stream":true,"include":[],"input":[]` + optional + `}`))
		if !ok || shape.Effort != "" {
			t.Fatal("optional field presence must not invent an effort or rejection")
		}
	}
	if _, ok := summarizeFirstRequest([]byte(`{"model":true}`)); ok {
		t.Fatal("malformed required fields must be rejected")
	}
}
