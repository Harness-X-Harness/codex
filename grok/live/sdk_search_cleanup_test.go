package live

import "testing"

func TestSearchCleanupRevalidatesObservedRawWithoutNewCredit(t *testing.T) {
	for _, tc := range []struct {
		name      string
		items     []string
		wantError bool
	}{
		{"observed_local", []string{`{"type":"custom_tool_call","id":"local","name":"apply_patch","input":"PRIVATE_INPUT"}`}, true},
		{"canonical_uncredited", []string{`{"type":"web_search_call","id":"call","status":"completed","action":{"type":"search","query":"PRIVATE_QUERY"}}`}, false},
		{"canonical_conflict", []string{`{"type":"web_search_call","id":"call","status":"completed","action":{"type":"search","query":"PRIVATE_QUERY"}}`, `{"type":"web_search_call","id":"call","status":"completed","action":{"type":"search","query":"PRIVATE_CHANGED"}}`}, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			evidence := Evidence{Stage: "search_turn_submitted"}
			result := SearchEvidence{Scenario: "web"}
			transport := &sdkProductObserver{}
			for _, item := range tc.items {
				params := []byte(`{"threadId":"thread","turnId":"turn","item":` + item + `}`)
				transport.pending = append(transport.pending, frame{Method: "rawResponseItem/completed", Params: params, size: len(params)})
			}
			probe := searchProbe{server: transport, threadID: "thread", turnID: "turn", scenario: "web", evidence: &evidence, result: &result, calls: map[string]map[string]string{"turn": {}}}
			err := probe.revalidate(nil)
			if (err != nil) != tc.wantError {
				t.Fatalf("validation=%v", err)
			}
			if result.CanonicalCalls != 0 || result.SettledTurns != 0 || evidence.Stage != "search_turn_submitted" || len(probe.calls["turn"]) != 0 {
				t.Fatal("cleanup created business credit")
			}
		})
	}
}
