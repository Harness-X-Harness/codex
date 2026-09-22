package facts

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"time"
)

const (
	factsEnv       = "GROK_FACTS"
	factsRecordEnv = "GROK_FACTS_RECORD"
	apiKeyEnv      = "GROK_API_KEY"

	requestTimeout   = 60 * time.Second
	errorPrefixLimit = 160
	maxResponseBytes = 8 << 20
)

// class is a recorded backend observation: accepted, ignored, or
// rejected:<status>/<error prefix>.
type class string

const (
	classAccepted class = "accepted"
	classIgnored  class = "ignored"
)

type profile struct {
	model   string
	baseURL string
}

type factClient struct {
	endpoint   string
	model      string
	apiKey     string
	httpClient *http.Client
}

func shippedGrokProfilePath() string {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		return filepath.Join("..", "dist", "config.toml.example")
	}
	return filepath.Join(filepath.Dir(file), "..", "dist", "config.toml.example")
}

func loadShippedProfile() (profile, error) {
	path := shippedGrokProfilePath()
	data, err := os.ReadFile(path)
	if err != nil {
		return profile{}, err
	}
	prof, err := profileFromTOML(data)
	if err != nil {
		return profile{}, fmt.Errorf("%s: %w", path, err)
	}
	return prof, nil
}

func profileFromTOML(data []byte) (profile, error) {
	model := topLevelTomlString(data, "model")
	baseURL := strings.TrimRight(tableString(data, "model_providers.grok", "base_url"), "/")
	if model == "" || baseURL == "" {
		return profile{}, fmt.Errorf("profile missing model or [model_providers.grok] base_url")
	}
	return profile{model: model, baseURL: baseURL}, nil
}

// topLevelTomlString reads a top-level key = "value" assignment. Keys inside
// [tables] are ignored. Surrounding quotes are stripped when present.
func topLevelTomlString(data []byte, key string) string {
	inTable := false
	for _, raw := range strings.Split(string(data), "\n") {
		line := strings.TrimSpace(raw)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		if strings.HasPrefix(line, "[") {
			inTable = true
			continue
		}
		if inTable {
			continue
		}
		name, rest, ok := strings.Cut(line, "=")
		if !ok || strings.TrimSpace(name) != key {
			continue
		}
		return tomlQuotedValue(rest)
	}
	return ""
}

func tomlQuotedValue(rest string) string {
	value := strings.TrimSpace(rest)
	if n := len(value); n >= 2 {
		quote := value[0]
		if (quote == '"' || quote == '\'') && value[n-1] == quote {
			return value[1 : n-1]
		}
	}
	return value
}

// tableString reads key = "value" from a TOML [table]. Nested tables with a
// different header end the scan. Surrounding quotes are stripped when present.
func tableString(data []byte, table, key string) string {
	want := "[" + table + "]"
	inTable := false
	for _, raw := range strings.Split(string(data), "\n") {
		line := strings.TrimSpace(raw)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		if strings.HasPrefix(line, "[") {
			inTable = line == want
			continue
		}
		if !inTable {
			continue
		}
		name, rest, ok := strings.Cut(line, "=")
		if !ok || strings.TrimSpace(name) != key {
			continue
		}
		return tomlQuotedValue(rest)
	}
	return ""
}

func newFactClient(apiKey string) (*factClient, error) {
	prof, err := loadShippedProfile()
	if err != nil {
		return nil, err
	}
	return &factClient{
		endpoint:   prof.baseURL + "/responses",
		model:      prof.model,
		apiKey:     apiKey,
		httpClient: &http.Client{Timeout: requestTimeout},
	}, nil
}

func (c *factClient) post(ctx context.Context, payload map[string]any) (int, []byte, error) {
	body := make(map[string]any, len(payload)+2)
	for k, v := range payload {
		body[k] = v
	}
	applyProfileModel(body, c.model)
	body["stream"] = false
	raw, err := json.Marshal(body)
	if err != nil {
		return 0, nil, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.endpoint, bytes.NewReader(raw))
	if err != nil {
		return 0, nil, err
	}
	req.Header.Set("Authorization", "Bearer "+c.apiKey)
	req.Header.Set("Content-Type", "application/json")
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return 0, nil, err
	}
	defer resp.Body.Close()
	limited, err := io.ReadAll(io.LimitReader(resp.Body, maxResponseBytes))
	if err != nil {
		return resp.StatusCode, nil, err
	}
	return resp.StatusCode, limited, nil
}

// applyProfileModel keeps an explicit probe model. Facts that omit model
// still use the shipped profile, which is the control route.
func applyProfileModel(body map[string]any, profileModel string) {
	if model, ok := body["model"].(string); ok && model != "" {
		return
	}
	body["model"] = profileModel
}

func responseModel(body []byte) string {
	var payload struct {
		Model string `json:"model"`
	}
	if json.Unmarshal(body, &payload) != nil {
		return ""
	}
	return payload.Model
}

func outputItems(body []byte) []map[string]any {
	var payload struct {
		Output []map[string]any `json:"output"`
	}
	if json.Unmarshal(body, &payload) != nil {
		return nil
	}
	return payload.Output
}

func firstFunctionCall(body []byte) map[string]any {
	for _, item := range outputItems(body) {
		if jsonString(item["type"]) == "function_call" {
			return item
		}
	}
	return nil
}

func classify(status int, body []byte) class {
	if status >= 200 && status < 300 {
		return classAccepted
	}
	return class(fmt.Sprintf("rejected:%d/%s", status, errorPrefix(body)))
}

func errorPrefix(body []byte) string {
	var payload map[string]any
	if err := json.Unmarshal(body, &payload); err != nil {
		return ""
	}
	if s := jsonString(payload["error"]); s != "" {
		return cutPrefix(collapseWhitespace(s), errorPrefixLimit)
	}
	if obj, ok := payload["error"].(map[string]any); ok {
		if s := jsonString(obj["message"]); s != "" {
			return cutPrefix(collapseWhitespace(s), errorPrefixLimit)
		}
	}
	if s := jsonString(payload["message"]); s != "" {
		return cutPrefix(collapseWhitespace(s), errorPrefixLimit)
	}
	return ""
}

func jsonString(v any) string {
	s, ok := v.(string)
	if !ok {
		return ""
	}
	return s
}

func collapseWhitespace(s string) string {
	return strings.Join(strings.Fields(s), " ")
}

func cutPrefix(s string, n int) string {
	if n <= 0 {
		return ""
	}
	runes := []rune(s)
	if len(runes) <= n {
		return s
	}
	return string(runes[:n])
}

func firstEncryptedContent(body []byte) string {
	var payload struct {
		Output []map[string]any `json:"output"`
	}
	if json.Unmarshal(body, &payload) != nil {
		return ""
	}
	for _, item := range payload.Output {
		if jsonString(item["type"]) != "reasoning" {
			continue
		}
		if s := jsonString(item["encrypted_content"]); s != "" {
			return s
		}
	}
	return ""
}

func observeEncryptedReasoning(body []byte) class {
	if firstEncryptedContent(body) != "" {
		return classAccepted
	}
	return classIgnored
}

func redact(s, secret string) string {
	if secret == "" {
		return s
	}
	return strings.ReplaceAll(s, secret, "[REDACTED]")
}
