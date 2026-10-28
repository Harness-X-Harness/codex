package live

import (
	"crypto/sha256"
	"strings"
	"testing"
)

func TestReasoningStructuredWholeAnswer(t *testing.T) {
	const token = "actual-tool-result"
	for _, tc := range []struct {
		name            string
		fragments       []string
		valid, recalled bool
	}{
		{"exact", []string{`{"token":"actual-tool-result"}`}, true, true},
		{"split", []string{`{"token":`, `"actual-tool-result"}`}, true, true},
		{"null", []string{`{"token":null}`}, true, false},
		{"empty", []string{`{"token":""}`}, true, false},
		{"wrong", []string{`{"token":"other"}`}, true, false},
		{"prose", []string{token}, false, false},
		{"substring", []string{`{"token":"prefix actual-tool-result"}`}, true, false},
		{"multiple", []string{`{"token":"actual-tool-result"}`, `{"token":null}`}, false, false},
		{"trailing_prose", []string{`{"token":"actual-tool-result"}`, `done`}, false, false},
		{"oversized", []string{strings.Repeat(" ", 4096) + `{"token":"actual-tool-result"}`}, false, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			for _, summary := range []bool{false, true} {
				probe := historyProbe{token: token}
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
	const token = "actual-tool-result"
	for _, tc := range []struct {
		name     string
		items    []historyItem
		conflict bool
	}{
		{"replay", []historyItem{{Type: "agentMessage", ID: "a", Phase: "final_answer", Text: `{"token":"actual-tool-result"}`}, {Type: "agentMessage", ID: "a", Phase: "final_answer", Text: `{"token":"actual-tool-result"}`}}, false},
		{"conflicting_fragments", []historyItem{{Type: "agentMessage", ID: "a", Phase: "final_answer", Text: `{"token":`}, {Type: "agentMessage", ID: "a", Phase: "final_answer", Text: `"actual-tool-result"}`}}, true},
		{"distinct_fragments", []historyItem{{Type: "agentMessage", ID: "a", Phase: "final_answer", Text: `{"token":`}, {Type: "agentMessage", ID: "b", Phase: "final_answer", Text: `"actual-tool-result"}`}}, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			probe := historyProbe{token: token}
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
