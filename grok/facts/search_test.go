package facts_test

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"testing"

	"github.com/Harness-X-Harness/codex/grok/facts"
)

func TestSearchPolicyPreservesEachRetainedWireFixture(t *testing.T) {
	cases := map[string]any{
		"web":          map[string]any{"type": "web_search"},
		"web_allowed":  map[string]any{"type": "web_search", "filters": map[string]any{"allowed_domains": []any{"reuters.com"}}},
		"web_excluded": map[string]any{"type": "web_search", "filters": map[string]any{"excluded_domains": []any{"example.com"}}},
		"x":            map[string]any{"type": "x_search"},
		"x_window":     map[string]any{"type": "x_search", "from_date": "2026-08-01", "to_date": "2026-08-15"},
	}
	for scenario, tool := range cases {
		t.Run(scenario, func(t *testing.T) {
			calls := 0
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls++
				var body map[string]any
				if json.NewDecoder(r.Body).Decode(&body) != nil || !reflect.DeepEqual(body["tools"], []any{tool}) || body["model"] != "explicit-fixture" || body["stream"] != false || r.Header.Get("Authorization") != "Bearer PRIVATE_CANARY" {
					t.Error("fixture wire changed")
				}
				_, _ = w.Write([]byte(`{"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}]}`))
			}))
			defer server.Close()
			probe, _ := facts.NewProbe(server.URL, "PRIVATE_CANARY")
			got, err := probe.SearchPolicy(context.Background(), "explicit-fixture", scenario)
			want := facts.SearchObservation{Observation: facts.Observation{Requests: 1, HTTPStatus: 200, Stage: "search_policy", Completed: true, TextBytes: 2}, Scenario: scenario}
			if err != nil || got != want || calls != 1 {
				t.Fatalf("policy = %+v, %v; calls=%d", got, err, calls)
			}
		})
	}
}

func TestSearchReplayPreservesSupportedCallsWithoutLocalOutput(t *testing.T) {
	cases := map[string]string{
		"web":               `{"type":"web_search_call","id":"PRIVATE_CALL","status":"completed","action":{"type":"search","query":"PRIVATE_QUERY"}}`,
		"x_keyword_search":  `{"type":"custom_tool_call","id":"PRIVATE_ID","call_id":"PRIVATE_CALL","status":"completed","name":"x_keyword_search","input":"PRIVATE_INPUT"}`,
		"x_semantic_search": `{"type":"custom_tool_call","id":"PRIVATE_ID","call_id":"PRIVATE_CALL","status":"completed","name":"x_semantic_search","input":"PRIVATE_INPUT"}`,
		"x_user_search":     `{"type":"custom_tool_call","id":"PRIVATE_ID","call_id":"PRIVATE_CALL","status":"completed","name":"x_user_search","input":"PRIVATE_INPUT"}`,
		"x_thread_fetch":    `{"type":"custom_tool_call","id":"PRIVATE_ID","call_id":"PRIVATE_CALL","status":"completed","name":"x_thread_fetch","input":"PRIVATE_INPUT"}`,
	}
	for name, call := range cases {
		t.Run(name, func(t *testing.T) {
			scenario := "x"
			if name == "web" {
				scenario = "web"
			}
			requests := 0
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				requests++
				var body struct {
					Input []json.RawMessage `json:"input"`
				}
				if json.NewDecoder(r.Body).Decode(&body) != nil {
					t.Error("invalid request")
				}
				output := `{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}`
				if requests == 1 {
					var wire map[string]any
					_ = json.Unmarshal([]byte(call), &wire)
					wire["vendor_debug"] = "PRIVATE_VENDOR"
					if action, ok := wire["action"].(map[string]any); ok {
						action["vendor_debug"] = "PRIVATE_VENDOR"
					}
					encoded, _ := json.Marshal(wire)
					output = string(encoded) + "," + output
				} else {
					var got, want any
					_ = json.Unmarshal(body.Input[0], &got)
					_ = json.Unmarshal([]byte(call), &want)
					if len(body.Input) != 2 || !reflect.DeepEqual(got, want) {
						t.Error("replay must preserve the admitted call and add no output")
					}
				}
				_, _ = w.Write([]byte(`{"status":"completed","output":[` + output + `]}`))
			}))
			defer server.Close()
			probe, _ := facts.NewProbe(server.URL, "PRIVATE_CANARY")
			got, err := probe.SearchReplay(context.Background(), "fixture", scenario)
			want := facts.SearchObservation{Observation: facts.Observation{Requests: 2, HTTPStatus: 200, Stage: "search_replay", Completed: true, TextBytes: 2, Replayed: true}, Scenario: scenario, HostedCalls: 1}
			if err != nil || got != want || requests != 2 {
				t.Fatalf("replay = %+v, %v; requests=%d", got, err, requests)
			}
			encoded, _ := json.Marshal(got)
			if strings.Contains(string(encoded), "PRIVATE_") {
				t.Fatal("private evidence leaked")
			}
		})
	}
}

func TestSearchReplayMissingOrInvalidEvidenceStopsWithoutResubmission(t *testing.T) {
	call := `{"type":"custom_tool_call","id":"PRIVATE_ID","call_id":"PRIVATE_CALL","status":"completed","name":"x_user_search","input":"PRIVATE_INPUT"}`
	cases := map[string]string{
		"absent":           "",
		"unsupported_name": strings.Replace(call, "x_user_search", "x_unknown", 1) + ",",
		"missing_id":       strings.Replace(call, `"id":"PRIVATE_ID",`, "", 1) + ",",
		"partial":          strings.Replace(call, "completed", "in_progress", 1) + ",",
		"namespace":        strings.Replace(call, `"type":`, `"namespace":"local","type":`, 1) + ",",
		"output":           call + `,{"type":"custom_tool_call_output","call_id":"PRIVATE_CALL","output":"PRIVATE_CANARY"},`,
		"local_function":   call + `,{"type":"function_call","call_id":"other","name":"local"},`,
	}
	for name, prefix := range cases {
		t.Run(name, func(t *testing.T) {
			requests := 0
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				requests++
				_, _ = w.Write([]byte(`{"status":"completed","output":[` + prefix + `{"type":"message","role":"assistant","content":[{"type":"output_text","text":"PRIVATE_TEXT"}]}]}`))
			}))
			defer server.Close()
			probe, _ := facts.NewProbe(server.URL, "PRIVATE_CANARY")
			got, err := probe.SearchReplay(context.Background(), "fixture", "x")
			if err == nil || requests != 1 || got.Requests != 1 || got.Replayed {
				t.Fatalf("invalid first result accepted: %+v, %v; requests=%d", got, err, requests)
			}
			if strings.Contains(err.Error(), "PRIVATE_") {
				t.Fatal("private error leaked")
			}
		})
	}
}

func TestSearchReplayFailurePreservesSafePriorEvidence(t *testing.T) {
	requests := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests++
		if requests == 2 {
			w.WriteHeader(422)
			_, _ = w.Write([]byte("PRIVATE_CANARY"))
			return
		}
		_, _ = w.Write([]byte(`{"status":"completed","output":[{"type":"web_search_call","id":"private","status":"completed","action":{"type":"open_page","url":"https://private.invalid"}},{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}]}`))
	}))
	defer server.Close()
	probe, _ := facts.NewProbe(server.URL, "PRIVATE_CANARY")
	got, err := probe.SearchReplay(context.Background(), "fixture", "web")
	want := facts.SearchObservation{Observation: facts.Observation{Requests: 2, HTTPStatus: 422, Stage: "search_replay", Replayed: true}, Scenario: "web", HostedCalls: 1}
	if err == nil || got != want || requests != 2 || strings.Contains(err.Error(), "PRIVATE_") {
		t.Fatalf("unsafe replay failure: %+v, %v", got, err)
	}
}

func TestSearchUnknownFixtureMakesNoRequest(t *testing.T) {
	requests := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { requests++ }))
	defer server.Close()
	probe, _ := facts.NewProbe(server.URL, "key")
	got, err := probe.SearchReplay(context.Background(), "fixture", "unknown")
	if err == nil || requests != 0 || got.Requests != 0 {
		t.Fatal("invalid fixture crossed HTTP boundary")
	}
}

func TestSearchReplayWebActionRepresentations(t *testing.T) {
	cases := []struct {
		name, action string
		valid        bool
	}{
		{"queries", `{"type":"search","queries":["first","second"]}`, true},
		{"open", `{"type":"open_page","url":"https://example.com"}`, true},
		{"find", `{"type":"find_in_page","url":"https://example.com","pattern":"text"}`, true},
		{"empty", `{"type":"search","query":"","queries":[]}`, true},
		{"action_only", `{"type":"search"}`, true},
		{"null_details", `{"type":"search","query":null,"queries":null}`, true},
		{"empty_queries", `{"type":"search","queries":[""]}`, true},
		{"open_missing_url", `{"type":"open_page"}`, true},
		{"open_empty_url", `{"type":"open_page","url":""}`, true},
		{"find_missing_details", `{"type":"find_in_page"}`, true},
		{"find_empty_details", `{"type":"find_in_page","url":"","pattern":""}`, true},
		{"url_type", `{"type":"open_page","url":1}`, false},
		{"pattern_type", `{"type":"find_in_page","pattern":1}`, false},
		{"null_action", `null`, false},
		{"missing_action_tag", `{}`, false},
		{"query_type", `{"type":"search","query":1,"queries":["text"]}`, false},
		{"queries_type", `{"type":"search","query":"text","queries":[1]}`, false},
		{"unknown", `{"type":"unknown","query":"text"}`, false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			requests := 0
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				requests++
				if requests == 2 {
					var body struct {
						Input []struct {
							Action any `json:"action"`
						} `json:"input"`
					}
					var expected any
					_ = json.Unmarshal([]byte(tc.action), &expected)
					if json.NewDecoder(r.Body).Decode(&body) != nil || len(body.Input) != 2 || !reflect.DeepEqual(body.Input[0].Action, expected) {
						t.Error("replay lost optional Web action fields")
					}
				}
				prefix := ""
				if requests == 1 {
					prefix = `{"type":"web_search_call","id":"private","status":"completed","action":` + tc.action + `},`
				}
				_, _ = w.Write([]byte(`{"status":"completed","output":[` + prefix + `{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}]}`))
			}))
			defer server.Close()
			probe, _ := facts.NewProbe(server.URL, "key")
			got, err := probe.SearchReplay(context.Background(), "fixture", "web")
			want := 1
			if tc.valid {
				want = 2
			}
			if (err == nil) != tc.valid || got.Replayed != tc.valid || requests != want {
				t.Fatalf("action completion = %+v, %v; requests=%d", got, err, requests)
			}
		})
	}
}

func TestSearchReplayRejectsContinuationLocalExecution(t *testing.T) {
	for _, kind := range []string{"function_call", "function_call_output", "custom_tool_call_output"} {
		t.Run(kind, func(t *testing.T) {
			requests := 0
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				requests++
				prefix := `{"type":"web_search_call","id":"private","status":"completed","action":{"type":"search","query":"private"}},`
				if requests == 2 {
					prefix = `{"type":"` + kind + `","id":"PRIVATE_ID","call_id":"PRIVATE_CALL","name":"local","output":"PRIVATE_CANARY"},`
				}
				_, _ = w.Write([]byte(`{"status":"completed","output":[` + prefix + `{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}]}`))
			}))
			defer server.Close()
			probe, _ := facts.NewProbe(server.URL, "PRIVATE_CANARY")
			got, err := probe.SearchReplay(context.Background(), "fixture", "web")
			want := facts.SearchObservation{Observation: facts.Observation{Requests: 2, HTTPStatus: 200, Stage: "search_replay", Completed: true, TextBytes: 2, Replayed: true}, Scenario: "web", HostedCalls: 1}
			if err == nil || got != want || requests != 2 {
				t.Fatalf("local replay accepted or failure evidence lost: %+v, %v; requests=%d", got, err, requests)
			}
			if strings.Contains(err.Error(), "PRIVATE_") {
				t.Fatal("private replay failure leaked")
			}
		})
	}
}

func TestSearchReplayRejectsNonHostedCustomCallsWithoutResubmission(t *testing.T) {
	for _, stage := range []int{1, 2} {
		for _, local := range []string{
			`{"type":"custom_tool_call","id":"PRIVATE_LOCAL","call_id":"PRIVATE_LOCAL_CALL","name":"apply_patch","input":"PRIVATE_INPUT"}`,
			`{"type":"custom_tool_call","id":"PRIVATE_LOCAL","call_id":"PRIVATE_LOCAL_CALL","namespace":"functions","name":"apply_patch","status":"completed","input":"PRIVATE_INPUT"}`,
			`{"type":"custom_tool_call","id":"PRIVATE_LOCAL","call_id":"PRIVATE_LOCAL_CALL","namespace":"functions","name":"x_keyword_search","status":"completed","input":"PRIVATE_INPUT"}`,
		} {
			requests := 0
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				requests++
				output := `{"type":"custom_tool_call","id":"PRIVATE_HOSTED","call_id":"PRIVATE_CALL","name":"x_keyword_search","status":"completed","input":"PRIVATE_INPUT"},`
				if requests == stage {
					output += local + ","
				}
				_, _ = w.Write([]byte(`{"status":"completed","output":[` + output + `{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}]}`))
			}))
			probe, _ := facts.NewProbe(server.URL, "PRIVATE_CANARY")
			got, err := probe.SearchReplay(context.Background(), "fixture", "x")
			server.Close()
			if err == nil || requests != stage || got.Requests != stage || got.Replayed != (stage == 2) || strings.Contains(err.Error(), "PRIVATE_") {
				t.Fatalf("non-hosted custom execution accepted or resubmitted: %+v, %v; requests=%d", got, err, requests)
			}
		}
	}
}

func TestSearchReplayAllowsOtherSupportedHostedKind(t *testing.T) {
	for _, scenario := range []string{"web", "x"} {
		t.Run(scenario, func(t *testing.T) {
			requests := 0
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				requests++
				_, _ = w.Write([]byte(`{"status":"completed","output":[{"type":"web_search_call","id":"PRIVATE_WEB","status":"completed","action":{"type":"search","query":"PRIVATE_QUERY"}},{"type":"custom_tool_call","id":"PRIVATE_X","call_id":"PRIVATE_CALL","name":"x_keyword_search","status":"completed","input":"PRIVATE_INPUT"},{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}]}`))
			}))
			defer server.Close()
			probe, _ := facts.NewProbe(server.URL, "PRIVATE_CANARY")
			got, err := probe.SearchReplay(context.Background(), "fixture", scenario)
			if err != nil || !got.Completed || !got.Replayed || got.HostedCalls != 1 || got.Requests != 2 || requests != 2 {
				t.Fatalf("supported coexisting hosted call rejected: %+v, %v", got, err)
			}
		})
	}
}
