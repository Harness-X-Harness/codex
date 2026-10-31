package live

import (
	"crypto/sha256"
	"strconv"
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
		{"long_whitespace", []string{strings.Repeat(" ", 8192) + `{"amount":14}`}, true, true},
		{"long_split", []string{strings.Repeat(" ", 8192) + `{"amount":`, `14}`}, true, true},
		{"long_wrong", []string{strings.Repeat(" ", 8192) + `{"amount":15}`}, true, false},
		{"long_trailing_prose", []string{strings.Repeat(" ", 8192) + `{"amount":14}`, `done`}, false, false},
		{"long_conflicting_fields", []string{strings.Repeat(" ", 8192) + `{"amount":14,"amount":15}`}, false, false},
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
					if err := probe.completedTurn(historyTurn{Status: "completed"}, &reply); err != nil {
						t.Fatal(err)
					}
				}
				if reply.formatValid != tc.valid || reply.recall != tc.recalled {
					t.Fatalf("summary=%t valid=%t recalled=%t", summary, reply.formatValid, reply.recall)
				}
				if reply.text.String() != strings.Join(tc.fragments, "\n") {
					t.Fatal("complete admitted reply was not retained")
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
			if err := probe.completedTurn(historyTurn{Status: "completed", Items: []historyItem{{ID: "answer", Type: "agentMessage", Phase: "final_answer", Text: `{"amount":` + amount + `}`}}}, &reply); err != nil {
				t.Fatal(err)
			}
			want := previous == "" && (amount == "14" || amount == "14.0") || previous != "" && (amount == "21" || amount == "21.0")
			if reply.recall != want {
				t.Fatalf("previous=%q amount=%s accepted=%t", previous, amount, reply.recall)
			}
		}
	}
}

func TestReasoningFragmentsAreEvaluatedAtTheCompletedBoundary(t *testing.T) {
	for _, phase := range []string{"", "final_answer"} {
		probe := historyProbe{unitPrice: 7}
		reply := historyReply{identities: make(map[[sha256.Size]byte]historyReplyOutcome)}
		for i := 0; i < 1024; i++ {
			if err := probe.completedItem(historyItem{Type: "agentMessage", ID: strconv.Itoa(i), Phase: phase, Text: strings.Repeat("x", 512)}, &reply); err != nil {
				t.Fatal(err)
			}
			if reply.evaluated || reply.recall || reply.formatValid {
				t.Fatal("a growing prefix was parsed or credited before the proof boundary")
			}
		}
		if err := probe.completedItem(historyItem{Type: "agentMessage", ID: "answer", Phase: "final_answer", Text: `{"amount":14}`}, &reply); err != nil {
			t.Fatal(err)
		}
		if err := probe.completedTurn(historyTurn{Status: "completed"}, &reply); err != nil {
			t.Fatal(err)
		}
		if !reply.evaluated || reply.recall != (phase == "") {
			t.Fatal("discarded legacy text or retained final fragments changed whole-answer semantics")
		}
		if err := probe.completedItem(historyItem{Type: "agentMessage", ID: "tail", Phase: "final_answer", Text: "wrong tail"}, &reply); err != nil {
			t.Fatal(err)
		}
		if reply.evaluated || reply.recall {
			t.Fatal("a new late fragment borrowed previously evaluated recall")
		}
		probe.evaluateReply(&reply)
		if reply.recall || reply.formatValid {
			t.Fatal("wrong tail accepted after the valid answer")
		}
	}
}
