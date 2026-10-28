package live

import (
	"crypto/sha256"
	"strings"
	"testing"
)

func TestReasoningStructuredWholeAnswer(t *testing.T) {
	const token = `{"unit_price_yuan":7}`
	for _, tc := range []struct {
		name            string
		fragments       []string
		valid, recalled bool
	}{
		{"exact", []string{`{"amount":14}`}, true, true},
		{"split", []string{`{"amount":`, `14}`}, true, true},
		{"null", []string{`{"amount":null}`}, true, false},
		{"empty", []string{`{"amount":""}`}, false, false},
		{"wrong", []string{`{"amount":15}`}, true, false},
		{"prose", []string{token}, false, false},
		{"substring", []string{`{"amount":"prefix 14"}`}, false, false},
		{"multiple", []string{`{"amount":14}`, `{"amount":null}`}, false, false},
		{"trailing_prose", []string{`{"amount":14}`, `done`}, false, false},
		{"oversized", []string{strings.Repeat(" ", 4096) + `{"amount":14}`}, false, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			for _, summary := range []bool{false, true} {
				probe := historyProbe{unitPrice: 7, toolResult: token}
				reply := historyReply{identities: map[[sha256.Size]byte]historyReplyOutcome{}}
				var items []historyItem
				for index, text := range tc.fragments {
					items = append(items, historyItem{Type: "agentMessage", ID: string(rune('a' + index)), Phase: "final_answer", Text: text})
				}
				if summary {
					if err := probe.completedTurn(historyTurn{Status: "completed", Items: items}, &reply); err != nil {
						t.Fatal(err)
					}
				} else {
					for _, item := range items {
						if err := probe.completedItem(item, &reply); err != nil {
							t.Fatal(err)
						}
					}
				}
				if reply.formatValid != tc.valid || reply.recall != tc.recalled {
					t.Fatalf("summary=%t valid=%t recalled=%t", summary, reply.formatValid, reply.recall)
				}
				if len(reply.text) > 4097 {
					t.Fatal("unbounded reply retention")
				}
			}
		})
	}
}

func TestReasoningStructuredSummaryIdentity(t *testing.T) {
	const token = `{"unit_price_yuan":7}`
	for _, tc := range []struct {
		name     string
		items    []historyItem
		conflict bool
	}{
		{"replay", []historyItem{{Type: "agentMessage", ID: "a", Phase: "final_answer", Text: `{"amount":14}`}, {Type: "agentMessage", ID: "a", Phase: "final_answer", Text: `{"amount":14}`}}, false},
		{"conflicting_fragments", []historyItem{{Type: "agentMessage", ID: "a", Phase: "final_answer", Text: `{"amount":`}, {Type: "agentMessage", ID: "a", Phase: "final_answer", Text: `14}`}}, true},
		{"distinct_fragments", []historyItem{{Type: "agentMessage", ID: "a", Phase: "final_answer", Text: `{"amount":`}, {Type: "agentMessage", ID: "b", Phase: "final_answer", Text: `14}`}}, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			probe := historyProbe{unitPrice: 7, toolResult: token}
			reply := historyReply{}
			err := probe.completedTurn(historyTurn{Status: "completed", Items: tc.items}, &reply)
			if tc.conflict {
				if DescribeFailure(err).Code != "assistant_identity_conflict" || !reply.ambiguous {
					t.Fatal("conflicting summary identity accepted")
				}
			} else if err != nil || !reply.recall || reply.ambiguous {
				t.Fatalf("valid summary rejected: %v", err)
			}
		})
	}
}

func TestReasoningPriceCalculationUsesCurrentQuantity(t *testing.T) {
	for _, previous := range []string{"", "previous-turn"} {
		for _, amount := range []string{"7", "14", "14.0", "21", "21.0", "null", `"21"`} {
			probe := historyProbe{unitPrice: 7, previousID: previous}
			reply := historyReply{}
			probe.addReplyFragment(historyItem{Phase: "final_answer", Text: `{"amount":` + amount + `}`}, &reply)
			want := previous == "" && (amount == "14" || amount == "14.0") || previous != "" && (amount == "21" || amount == "21.0")
			if reply.recall != want {
				t.Fatalf("previous=%q amount=%s accepted=%t", previous, amount, reply.recall)
			}
		}
	}
}
