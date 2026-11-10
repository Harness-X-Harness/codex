package facts_test

import (
	"context"
	"encoding/json"
	"github.com/Harness-X-Harness/codex/grok/facts"
	"net/http"
	"net/http/httptest"
	"reflect"
	"testing"
)

func TestShippedRoutesKeepRequestSlugsAndStopOnFailure(t *testing.T) {
	for _, fail := range []bool{false, true} {
		models := []string{}
		server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			var body struct{ Model string }
			_ = json.NewDecoder(r.Body).Decode(&body)
			models = append(models, body.Model)
			if fail {
				w.WriteHeader(500)
				return
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"model": "grok-4.7-build", "status": "completed", "output": []any{map[string]any{"type": "message", "role": "assistant", "content": []any{map[string]any{"type": "output_text", "text": "ok"}}}}})
		}))
		probe, err := facts.NewProbe(server.URL, "fixture-key")
		if err != nil {
			t.Fatal(err)
		}
		got, err := probe.ShippedRoutes(context.Background())
		server.Close()
		want := []string{"grok-4.7", "grok-4.6"}
		if fail {
			want = want[:1]
		}
		if !reflect.DeepEqual(models, want) || (err != nil) != fail || len(got) != len(want) {
			t.Fatal("shipped route request budget or identity mismatch")
		}
		for i, route := range got {
			if route.Model != want[i] || route.Observation.Requests != 1 || route.Observation.ReturnedModelMatches {
				t.Fatal("response alias overwrote source policy")
			}
		}
	}
}
