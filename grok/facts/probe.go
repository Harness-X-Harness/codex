// Package facts records bounded backend observations, independently of product Live.
package facts

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// Observation contains only safe metadata; it never contains backend output.
type Observation struct {
	Requests             int
	HTTPStatus           int
	Stage                string
	Completed            bool
	TextBytes            int
	EncryptedItems       int
	Replayed             bool
	ReturnedModelMatches bool
}

// Probe observes one explicitly selected HTTP Responses endpoint.
type Probe struct {
	endpoint string
	apiKey   string
	client   *http.Client
}

// NewProbe binds an endpoint without allowing redirects to change its boundary.
func NewProbe(endpoint, apiKey string) (*Probe, error) {
	parsed, err := url.Parse(endpoint)
	if err != nil || parsed.Host == "" || (parsed.Scheme != "http" && parsed.Scheme != "https") || parsed.User != nil || parsed.Fragment != "" || apiKey == "" {
		return nil, errors.New("invalid probe configuration")
	}
	return &Probe{endpoint: endpoint, apiKey: apiKey, client: &http.Client{
		Timeout: 60 * time.Second,
		CheckRedirect: func(*http.Request, []*http.Request) error {
			return errors.New("redirect rejected")
		},
	}}, nil
}

// Text initiates one non-streaming text request for the explicit fixture model.
func (p *Probe) Text(ctx context.Context, model string) (Observation, error) {
	observation := Observation{Stage: "text"}
	if strings.TrimSpace(model) == "" {
		return observation, errors.New("missing fixture model")
	}
	body, _ := json.Marshal(map[string]any{
		"model": model, "stream": false,
		"input": []any{map[string]any{"type": "message", "role": "user", "content": []any{map[string]any{"type": "input_text", "text": "Reply with the single word ok."}}}},
	})
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, p.endpoint, bytes.NewReader(body))
	if err != nil {
		return observation, errors.New("request construction failed")
	}
	request.Header.Set("Authorization", "Bearer "+p.apiKey)
	request.Header.Set("Content-Type", "application/json")
	observation.Requests++
	response, err := p.client.Do(request)
	if err != nil {
		return observation, errors.New("HTTP transport failed")
	}
	defer response.Body.Close()
	observation.HTTPStatus = response.StatusCode
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return observation, errors.New("HTTP response rejected")
	}
	raw, err := io.ReadAll(io.LimitReader(response.Body, (8<<20)+1))
	if err != nil || len(raw) > 8<<20 {
		return observation, errors.New("response unreadable or oversized")
	}
	var result struct {
		Status string `json:"status"`
		Model  string `json:"model"`
		Output []struct {
			Type    string `json:"type"`
			Role    string `json:"role"`
			Content []struct {
				Type string `json:"type"`
				Text string `json:"text"`
			} `json:"content"`
		} `json:"output"`
	}
	if json.Unmarshal(raw, &result) != nil || result.Status != "completed" {
		return observation, errors.New("complete response absent")
	}
	for _, item := range result.Output {
		if item.Type == "message" && item.Role == "assistant" {
			for _, part := range item.Content {
				if part.Type == "output_text" && strings.TrimSpace(part.Text) != "" {
					observation.TextBytes += len(part.Text)
				}
			}
		}
	}
	if observation.TextBytes == 0 {
		return observation, errors.New("assistant text absent")
	}
	observation.Completed = true
	observation.ReturnedModelMatches = result.Model == model
	return observation, nil
}

// EncryptedReplay observes one initial request and one opaque typed-history replay.
func (p *Probe) EncryptedReplay(ctx context.Context, model string) (Observation, error) {
	return Observation{}, errors.New("encrypted replay not implemented")
}
