package live_test

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/live"
)

func TestReasoningStructuredPersistedEvidence(t *testing.T) {
	options := fixtureOptions(t, "history:turn")
	dir := t.TempDir()
	options.Diagnostics = live.NewDiagnostics(dir, options.APIKey)
	options.Diagnostics.Record("privacy_canary", map[string]any{"authorization": "AUTH_CANARY", "encrypted_content": "CIPHER_CANARY"})
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	_, err := live.ReasoningHistory(ctx, options)
	options.Diagnostics.Close()
	if err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(filepath.Join(dir, "events.jsonl"))
	if err != nil {
		t.Fatal(err)
	}
	var toolValue string
	schemas, replies := 0, 0
	for _, line := range strings.Split(strings.TrimSpace(string(data)), "\n") {
		var event struct {
			Kind   string
			Fields map[string]json.RawMessage
		}
		if err := json.Unmarshal([]byte(line), &event); err != nil {
			t.Fatal(err)
		}
		switch event.Kind {
		case "history_tool_output":
			if err := json.Unmarshal(event.Fields["text"], &toolValue); err != nil {
				t.Fatal(err)
			}
		case "turn_input":
			var schema struct {
				Properties map[string]any
				Required   []string
				Additional bool `json:"additionalProperties"`
			}
			if err := json.Unmarshal(event.Fields["output_schema"], &schema); err != nil {
				t.Fatal(err)
			}
			if schema.Properties["amount"] == nil || len(schema.Required) != 1 || schema.Required[0] != "amount" || schema.Additional {
				t.Fatal("actual amount schema not retained")
			}
			schemas++
		case "oracle_result":
			var raw string
			if err := json.Unmarshal(event.Fields["reply"], &raw); err != nil {
				t.Fatal(err)
			}
			var result struct{ Amount int }
			var price struct {
				UnitPrice int `json:"unit_price_yuan"`
			}
			if err := json.Unmarshal([]byte(raw), &result); err != nil {
				t.Fatal("raw structured reply lost")
			}
			if json.Unmarshal([]byte(toolValue), &price) != nil || price.UnitPrice == 0 || result.Amount != (replies+2)*price.UnitPrice {
				t.Fatal("actual task amount not retained")
			}
			replies++
		}
	}
	if schemas != 2 || replies != 2 {
		t.Fatalf("schemas=%d replies=%d", schemas, replies)
	}
	for _, secret := range []string{options.APIKey, historyCanary, "AUTH_CANARY", "CIPHER_CANARY"} {
		if strings.Contains(string(data), secret) {
			t.Fatal("private material retained")
		}
	}
}

// This is a deterministic observer test. Telemetry completeness is deliberately
// absent from the product verdict, for both successful and rejected tasks.
func TestReasoningDiagnosticFailureDoesNotChangeOutcome(t *testing.T) {
	for _, test := range []struct{ mode, code string }{
		{"turn", ""}, {"no_request", "history_reply_invalid"},
		{"no_request_amount", "history_lookup_not_observed"}, {"wrong_reply", "history_reply_invalid"},
	} {
		for _, sink := range []string{"disabled", "closed", "unwritable"} {
			t.Run(test.mode+"/"+sink, func(t *testing.T) {
				options := fixtureOptions(t, "history:"+test.mode)
				if sink != "disabled" {
					path := t.TempDir()
					if sink == "unwritable" {
						path = filepath.Join(path, "occupied")
						if err := os.WriteFile(path, []byte("not a directory"), 0600); err != nil {
							t.Fatal(err)
						}
					}
					options.Diagnostics = live.NewDiagnostics(path, options.APIKey)
					if sink == "closed" {
						options.Diagnostics.Close()
					}
				}
				ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
				defer cancel()
				got, err := live.ReasoningHistory(ctx, options)
				options.Diagnostics.Close()
				if (err == nil) != (test.code == "") || got.Completed != (test.code == "") {
					t.Fatalf("sink changed product completion: %+v %v", got, err)
				}
				if test.mode == "no_request_amount" && (got.ToolCalls != 0 || got.CompletedTools != 0 || got.Turns != 1 || got.FirstRecall) {
					t.Fatal("unobserved lookup invented a price, accepted call or continuation")
				}
				if err != nil && live.DescribeFailure(err).Code != test.code {
					t.Fatalf("sink changed failure boundary: %+v", live.DescribeFailure(err))
				}
			})
		}
	}
}
