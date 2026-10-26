//go:build integration

package integration_test

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"image/png"
	"io"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

// Uses the same selected binary/snapshot prerequisite as the existing owners.
// The HTTP peer is controlled; llm-go remains the only product driver.
func TestAttachedImagePinnedFixture(t *testing.T) {
	integrationObservation(t)
	subject := nativeSubject(t)
	var mu sync.Mutex
	requests, imageTurns := 0, 0
	invalid := false
	var original string
	backend := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		defer r.Body.Close()
		mu.Lock()
		defer mu.Unlock()
		requests++
		var body struct {
			Model string
			Input []struct {
				Role    string
				Content []struct {
					Type, Text string
					ImageURL   string `json:"image_url"`
				}
			}
			Text   json.RawMessage
			Tools  []json.RawMessage
			Stream bool
		}
		if json.NewDecoder(io.LimitReader(r.Body, 2<<20)).Decode(&body) != nil || r.Method != "POST" || r.URL.Path != "/v1/responses" || r.Header.Get("Authorization") != "Bearer fixture-key" || body.Model != providerfixture.PinnedModel || !body.Stream || len(body.Tools) != 0 || requests > 2 {
			invalid = true
		}
		var text struct {
			Format struct {
				Type   string
				Strict bool
				Schema any
			}
		}
		var expectedSchema any
		_ = json.Unmarshal([]byte(`{"type":"object","properties":{"color":{"type":["string","null"]},"shape":{"type":["string","null"]}},"required":["color","shape"],"additionalProperties":false}`), &expectedSchema)
		if json.Unmarshal(body.Text, &text) != nil || text.Format.Type != "json_schema" || !text.Format.Strict || !reflect.DeepEqual(text.Format.Schema, expectedSchema) {
			invalid = true
		}
		images := 0
		for _, item := range body.Input {
			for _, part := range item.Content {
				if part.Type == "input_image" {
					images++
					raw, err := base64.StdEncoding.DecodeString(strings.TrimPrefix(part.ImageURL, "data:image/png;base64,"))
					if err != nil {
						invalid = true
						continue
					}
					picture, err := png.Decode(bytes.NewReader(raw))
					if err != nil || picture.Bounds().Dx() != 480 || picture.Bounds().Dy() != 240 {
						invalid = true
					}
					if original == "" {
						original = part.ImageURL
					} else if original != part.ImageURL {
						invalid = true
					}
				}
			}
		}
		if images != 1 {
			invalid = true
		} else {
			imageTurns++
		}
		if invalid {
			w.WriteHeader(http.StatusBadRequest)
			_, _ = io.WriteString(w, `{"error":{"message":"controlled fixture mismatch"}}`)
			return
		}
		answer := `{"color":"red","shape":"circle"}`
		if requests == 2 {
			answer = `{"color":"blue","shape":"square"}`
		}
		events := []any{map[string]any{"type": "response.created", "response": map[string]any{"id": fmt.Sprintf("attached-%d", requests)}}}
		events = append(events, assistantEvents(0, fmt.Sprintf("attached-answer-%d", requests), answer)...)
		events = append(events, map[string]any{"type": "response.completed", "response": map[string]any{"id": fmt.Sprintf("attached-%d", requests)}})
		_ = writeSSE(w, events)
	}))
	defer backend.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	got, err := live.AttachedImageHistory(ctx, live.Options{Subject: subject, Model: providerfixture.PinnedModel, BaseURL: backend.URL + "/v1", APIKey: "fixture-key"})
	mu.Lock()
	defer mu.Unlock()
	if err != nil || invalid || requests != 2 || imageTurns != 2 || !got.Completed || !got.FirstCompleted || !got.Recalled {
		t.Fatalf("attached image composition not proven: requests=%d image_turns=%d invalid=%t evidence=%+v error=%v", requests, imageTurns, invalid, got, err)
	}
}
