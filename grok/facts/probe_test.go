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
