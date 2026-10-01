package facts_test

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"reflect"
	"testing"

	"github.com/Harness-X-Harness/codex/grok/facts"
)

func TestProbeTextUsesExplicitFixtureAndObservesCompletion(t *testing.T) {
	var requests []map[string]any
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Error("invalid request JSON")
		}
		requests = append(requests, body)
		if r.Method != http.MethodPost || r.Header.Get("Authorization") != "Bearer private-key" {
			t.Error("wrong method or authentication")
		}
		_, _ = w.Write([]byte(`{"status":"completed","model":"backend-alias","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}]}`))
	}))
	defer server.Close()
	probe, err := facts.NewProbe(server.URL, "private-key")
	if err != nil {
		t.Fatal(err)
	}
	got, err := probe.Text(context.Background(), "fixture-pinned")
	if err != nil {
		t.Fatal(err)
	}
	want := facts.Observation{Requests: 1, HTTPStatus: 200, Stage: "text", Completed: true, TextBytes: 2}
	if got != want {
		t.Fatalf("observation = %+v, want %+v", got, want)
	}
	wantRequests := []map[string]any{{
		"model": "fixture-pinned", "stream": false,
		"input": []any{map[string]any{"type": "message", "role": "user", "content": []any{map[string]any{"type": "input_text", "text": "Reply with the single word ok."}}}},
	}}
	if !reflect.DeepEqual(requests, wantRequests) {
		t.Fatal("request did not preserve the fixture's explicit model/text input")
	}
}

func TestProbeReplayPreservesOpaqueValueAndTypedContent(t *testing.T) {
	var requests []map[string]any
	const opaque = "opaque+/=\nunchanged"
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Error("invalid request JSON")
		}
		requests = append(requests, body)
		output := []any{map[string]any{"type": "message", "role": "assistant", "content": []any{map[string]any{"type": "output_text", "text": "ok"}}}}
		if len(requests) == 1 {
			output = append(output, map[string]any{"type": "reasoning", "encrypted_content": opaque})
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"status": "completed", "model": "fixture-primary", "output": output})
	}))
	defer server.Close()
	probe, err := facts.NewProbe(server.URL, "private-key")
	if err != nil {
		t.Fatal(err)
	}
	got, err := probe.EncryptedReplay(context.Background(), "fixture-primary")
	if err != nil {
		t.Fatal(err)
	}
	want := facts.Observation{Requests: 2, HTTPStatus: 200, Stage: "replay", Completed: true, TextBytes: 2, EncryptedItems: 1, Replayed: true, ReturnedModelMatches: true}
	if got != want {
		t.Fatalf("observation = %+v, want %+v", got, want)
	}
	textInput := []any{map[string]any{"type": "message", "role": "user", "content": []any{map[string]any{"type": "input_text", "text": "Reply with the single word ok."}}}}
	wantRequests := []map[string]any{
		{"model": "fixture-primary", "stream": false, "input": textInput, "include": []any{"reasoning.encrypted_content"}},
		{"model": "fixture-primary", "stream": false, "input": append([]any{map[string]any{"type": "reasoning", "encrypted_content": opaque, "summary": []any{}, "content": []any{map[string]any{"type": "reasoning_text", "text": "x"}}}}, textInput...)},
	}
	if !reflect.DeepEqual(requests, wantRequests) {
		t.Fatal("replay did not preserve the opaque value and typed content")
	}
}

func TestProbeTextRequiresSemanticCompletion(t *testing.T) {
	cases := []struct {
		name, body string
		complete   bool
	}{
		{"completed_item_without_envelope_status", `{"output":[{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"ok"}]}]}`, true},
		{"partial_response", `{"status":"in_progress","output":[{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"ok"}]}]}`, false},
		{"partial_item", `{"status":"completed","output":[{"type":"message","role":"assistant","status":"in_progress","content":[{"type":"output_text","text":"ok"}]}]}`, false},
		{"error_with_text", `{"status":"completed","error":{"message":"private-key"},"output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}]}`, false},
		{"incomplete_details", `{"status":"completed","incomplete_details":{"reason":"private-key"},"output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}]}`, false},
		{"text_without_terminal", `{"output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}]}`, false},
		{"empty_output", `{"status":"completed","output":[]}`, false},
		{"user_text", `{"status":"completed","output":[{"type":"message","role":"user","content":[{"type":"output_text","text":"ok"}]}]}`, false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				_, _ = w.Write([]byte(tc.body))
			}))
			defer server.Close()
			probe, err := facts.NewProbe(server.URL, "private-key")
			if err != nil {
				t.Fatal(err)
			}
			got, err := probe.Text(context.Background(), "fixture")
			want := facts.Observation{Requests: 1, HTTPStatus: 200, Stage: "text"}
			if tc.complete {
				want.Completed, want.TextBytes = true, 2
			}
			if (err == nil) != tc.complete || got != want {
				t.Fatalf("observation = %+v, error = %v; want %+v", got, err, want)
			}
		})
	}
}
