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
	observation, _, err := p.post(ctx, model, "text", textInput(), nil)
	return observation, err
}

// EncryptedReplay observes one initial request and one opaque typed-history replay.
func (p *Probe) EncryptedReplay(ctx context.Context, model string) (Observation, error) {
	initial, opaque, err := p.post(ctx, model, "initial", textInput(), []string{"reasoning.encrypted_content"})
	if err != nil {
		return initial, err
	}
	if opaque == "" {
		return initial, errors.New("encrypted reasoning absent")
	}
	input := append([]any{map[string]any{
		"type": "reasoning", "encrypted_content": opaque, "summary": []any{},
		"content": []any{map[string]any{"type": "reasoning_text", "text": "x"}},
	}}, textInput()...)
	replay, _, err := p.post(ctx, model, "replay", input, nil)
	replay.Requests += initial.Requests
	replay.EncryptedItems += initial.EncryptedItems
	replay.Replayed = true
	return replay, err
}

func textInput() []any {
	return []any{map[string]any{"type": "message", "role": "user", "content": []any{map[string]any{"type": "input_text", "text": "Reply with the single word ok."}}}}
}

func (p *Probe) post(ctx context.Context, model, stage string, input []any, include []string) (Observation, string, error) {
	observation := Observation{Stage: stage}
	if strings.TrimSpace(model) == "" {
		return observation, "", errors.New("missing fixture model")
	}
	payload := map[string]any{"model": model, "stream": false, "input": input}
	if len(include) != 0 {
		payload["include"] = include
	}
	body, err := json.Marshal(payload)
	if err != nil {
		return observation, "", errors.New("request encoding failed")
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, p.endpoint, bytes.NewReader(body))
	if err != nil {
		return observation, "", errors.New("request construction failed")
	}
	request.Header.Set("Authorization", "Bearer "+p.apiKey)
	request.Header.Set("Content-Type", "application/json")
	observation.Requests++
	response, err := p.client.Do(request)
	if err != nil {
		return observation, "", errors.New("HTTP transport failed")
	}
	defer response.Body.Close()
	observation.HTTPStatus = response.StatusCode
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return observation, "", errors.New("HTTP response rejected")
	}
	raw, err := io.ReadAll(io.LimitReader(response.Body, (8<<20)+1))
	if err != nil || len(raw) > 8<<20 {
		return observation, "", errors.New("response unreadable or oversized")
	}
	var result struct {
		Status            string          `json:"status"`
		Model             string          `json:"model"`
		Error             json.RawMessage `json:"error"`
		IncompleteDetails json.RawMessage `json:"incomplete_details"`
		Output            []struct {
			Type             string `json:"type"`
			Role             string `json:"role"`
			Status           string `json:"status"`
			EncryptedContent string `json:"encrypted_content"`
			Content          []struct {
				Type string `json:"type"`
				Text string `json:"text"`
			} `json:"content"`
		} `json:"output"`
	}
	if json.Unmarshal(raw, &result) != nil || (result.Status != "" && result.Status != "completed") {
		return observation, "", errors.New("complete response absent")
	}
	for _, field := range []json.RawMessage{result.Error, result.IncompleteDetails} {
		if len(field) != 0 && string(bytes.TrimSpace(field)) != "null" {
			return observation, "", errors.New("response reports failure or incompletion")
		}
	}
	// A completed envelope permits omitted message status. Without it, every
	// assistant message must establish its own completion before text can count.
	for _, item := range result.Output {
		if (item.Status != "" && item.Status != "completed") ||
			(item.Type == "message" && item.Role == "assistant" && result.Status == "" && item.Status != "completed") {
			return observation, "", errors.New("complete output absent")
		}
	}
	var opaque string
	for _, item := range result.Output {
		if item.Type == "reasoning" && item.EncryptedContent != "" {
			observation.EncryptedItems++
			if opaque == "" {
				opaque = item.EncryptedContent
			}
		}
		if item.Type == "message" && item.Role == "assistant" {
			for _, part := range item.Content {
				if part.Type == "output_text" && strings.TrimSpace(part.Text) != "" {
					observation.TextBytes += len(part.Text)
				}
			}
		}
	}
	if observation.TextBytes == 0 {
		return observation, "", errors.New("assistant text absent")
	}
	observation.Completed = true
	observation.ReturnedModelMatches = result.Model == model
	return observation, opaque, nil
}
