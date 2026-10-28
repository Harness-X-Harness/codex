package live

import (
	"crypto/sha256"
	"encoding/json"
	"testing"
)

// MessagePhase is owned by protocol/src/models.rs: commentary and partial are
// nonterminal; missing phase keeps the provider's legacy compatibility path.
func TestHistoryReplyPhaseSelection(t *testing.T) {
	const token = `{"unit_price_yuan":7}`
	item := func(id, phase, text string) historyItem {
		return historyItem{Type: "agentMessage", ID: id, Phase: phase, Text: historyTestJSON(text)}
	}
	for _, tc := range []struct {
		name  string
		items []historyItem
		want  bool
	}{
		{"final_then_empty_final", []historyItem{item("final", "final_answer", token), item("empty", "final_answer", "")}, true},
		{"final_then_blank_legacy", []historyItem{item("final", "final_answer", token), item("empty", "", " \n ")}, true},
		{"empty_final_before_legacy", []historyItem{item("empty", "final_answer", ""), item("legacy", "", token)}, true},
		{"legacy_then_empty_final", []historyItem{item("legacy", "", token), item("empty", "final_answer", " \n ")}, true},
		{"all_empty", []historyItem{item("empty", "final_answer", ""), item("blank", "", " \n ")}, false},
		{"final_then_commentary", []historyItem{item("final", "final_answer", token), item("tail", "commentary", "progress")}, true},
		{"final_then_partial", []historyItem{item("final", "final_answer", token), item("tail", "partial_answer", "partial")}, true},
		{"final_before_legacy", []historyItem{item("final", "final_answer", token), item("legacy", "", "wrong")}, true},
		{"final_then_explanation", []historyItem{item("first", "final_answer", token), item("last", "final_answer", "This is the recalled result.")}, false},
		{"legacy_then_explanation", []historyItem{item("first", "", token), item("last", "", "This is the recalled result.")}, false},
		{"legacy_cannot_replace_explicit_final", []historyItem{item("legacy", "", token), item("final", "final_answer", "not recalled")}, false},
		{"legacy_then_commentary", []historyItem{item("legacy", "", token), item("tail", "commentary", "progress")}, true},
		{"only_commentary", []historyItem{item("tail", "commentary", token)}, false},
		{"only_partial", []historyItem{item("partial", "partial_answer", token)}, false},
	} {
		for _, summary := range []bool{false, true} {
			name := "items"
			if summary {
				name = "summary"
			}
			t.Run(tc.name+"/"+name, func(t *testing.T) {
				probe := historyProbe{unitPrice: 7, toolResult: token}
				reply := historyReply{identities: make(map[[sha256.Size]byte]historyReplyOutcome)}
				if summary {
					if err := probe.completedTurn(historyTurn{Status: "completed", Items: tc.items}, &reply); err != nil {
						t.Fatal(err)
					}
				} else {
					for _, value := range tc.items {
						if err := probe.completedItem(value, &reply); err != nil {
							t.Fatal(err)
						}
					}
					if err := probe.completedTurn(historyTurn{Status: "completed"}, &reply); err != nil {
						t.Fatal(err)
					}
				}
				if !reply.completed || (reply.recall && !reply.ambiguous) != tc.want {
					t.Fatal("phase selection credited nonfinal or lost eligible final content")
				}
			})
		}
	}
	for _, summary := range [][]historyItem{
		{item("summary", "final_answer", "")},
		{item("summary", "", " \n ")},
		{item("summary", "commentary", token)},
		{item("summary", "", "wrong")},
	} {
		probe := historyProbe{unitPrice: 7, toolResult: token}
		reply := historyReply{identities: make(map[[sha256.Size]byte]historyReplyOutcome)}
		if err := probe.completedItem(item("earlier", "final_answer", token), &reply); err != nil {
			t.Fatal(err)
		}
		if err := probe.completedTurn(historyTurn{Status: "completed", Items: summary}, &reply); err != nil {
			t.Fatal(err)
		}
		if !reply.authoritative || reply.recall {
			t.Fatal("authoritative summary borrowed an earlier notification result")
		}
	}
}

func TestHistoryReplyPhaseIdentityConflict(t *testing.T) {
	const token = `{"unit_price_yuan":7}`
	probe := historyProbe{unitPrice: 7, toolResult: token}
	reply := historyReply{identities: make(map[[sha256.Size]byte]historyReplyOutcome)}
	for _, item := range []historyItem{
		{Type: "agentMessage", ID: "a", Phase: "final_answer", Text: historyTestJSON(token)},
		{Type: "agentMessage", ID: "a", Text: historyTestJSON(token)},
		{Type: "agentMessage", ID: "b", Text: "wrong"},
	} {
		if err := probe.completedItem(item, &reply); err != nil {
			t.Fatal(err)
		}
	}
	if !reply.ambiguous {
		t.Fatal("same-ID explicit-final/legacy conflict was treated as replay")
	}
	if err := probe.completedTurn(historyTurn{Status: "completed", Items: []historyItem{
		{Type: "agentMessage", ID: "authoritative", Phase: "final_answer", Text: historyTestJSON(token)},
	}}, &reply); err != nil {
		t.Fatal(err)
	}
	if !reply.authoritative || reply.ambiguous || !reply.recall {
		t.Fatal("authoritative summary did not settle provisional phase conflict")
	}
}

func TestHistoryEmptyReplyIdentity(t *testing.T) {
	const token = `{"unit_price_yuan":7}`
	for _, summary := range []bool{false, true} {
		name := "items"
		if summary {
			name = "summary"
		}
		t.Run("missing_identity/"+name, func(t *testing.T) {
			probe := historyProbe{unitPrice: 7, toolResult: token}
			reply := historyReply{identities: make(map[[sha256.Size]byte]historyReplyOutcome)}
			item := historyItem{Type: "agentMessage", Phase: "final_answer"}
			var err error
			if summary {
				err = probe.completedTurn(historyTurn{Status: "completed", Items: []historyItem{item}}, &reply)
			} else {
				err = probe.completedItem(item, &reply)
			}
			if DescribeFailure(err).Code != "assistant_identity_missing" {
				t.Fatal("empty text bypassed required identity")
			}
		})
	}
	for _, kind := range []string{"content", "phase"} {
		t.Run("same_id_"+kind+"_conflict", func(t *testing.T) {
			probe := historyProbe{unitPrice: 7, toolResult: token}
			reply := historyReply{identities: make(map[[sha256.Size]byte]historyReplyOutcome)}
			empty := historyItem{Type: "agentMessage", ID: "empty", Phase: "final_answer"}
			changed := empty
			if kind == "content" {
				changed.Text = "wrong"
			} else {
				changed.Phase = ""
			}
			for _, item := range []historyItem{empty, changed, {Type: "agentMessage", ID: "valid", Phase: "final_answer", Text: historyTestJSON(token)}} {
				if err := probe.completedItem(item, &reply); err != nil {
					t.Fatal(err)
				}
			}
			if !reply.ambiguous {
				t.Fatal("empty contribution skipped same-ID content/phase conflict")
			}
		})
	}
}

func historyTestJSON(token string, quantities ...int) string {
	quantity := 2
	if len(quantities) != 0 {
		quantity = quantities[0]
	}
	if token == "" || token == " \n " {
		return token
	}
	var price struct {
		UnitPrice int `json:"unit_price_yuan"`
	}
	if json.Unmarshal([]byte(token), &price) == nil && price.UnitPrice > 0 {
		data, _ := json.Marshal(map[string]int{"amount": quantity * price.UnitPrice})
		return string(data)
	}
	data, _ := json.Marshal(map[string]string{"amount": token})
	return string(data)
}
