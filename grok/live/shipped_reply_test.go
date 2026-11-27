package live

import (
	"fmt"
	"strings"
	"testing"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func TestFinalProductReplyEmptyContributions(t *testing.T) {
	message := func(id, phase, text string) protocolv2.ThreadItem {
		item := protocolv2.ThreadItemAgentMessage{ID: id, Text: text}
		if phase != "" {
			item.Phase = protocolv2.Value(protocolv2.MessagePhase(phase))
		}
		return protocolv2.NewThreadItemAgentMessage(item)
	}
	valid := message("answer", "final_answer", "answer")
	for _, test := range []struct {
		name  string
		items []protocolv2.ThreadItem
		valid bool
	}{
		{"anonymous_commentary_before_final", []protocolv2.ThreadItem{message("", "commentary", "Progress."), valid}, true},
		{"anonymous_legacy_before_final", []protocolv2.ThreadItem{message("", "", "Legacy."), valid}, false},
		{"empty_legacy_before_final", []protocolv2.ThreadItem{message("empty", "", " \n "), valid}, true},
		{"empty_final_before_final", []protocolv2.ThreadItem{message("empty", "final_answer", ""), valid}, true},
		{"duplicate_empty_before_final", []protocolv2.ThreadItem{message("empty", "final_answer", ""), message("empty", "final_answer", ""), valid}, true},
		{"all_empty", []protocolv2.ThreadItem{message("a", "", ""), message("b", "final_answer", " \n ")}, false},
		{"missing_identity", []protocolv2.ThreadItem{message("", "final_answer", ""), valid}, false},
		{"same_id_empty_then_content", []protocolv2.ThreadItem{message("answer", "final_answer", ""), valid}, false},
		{"same_id_content_then_empty", []protocolv2.ThreadItem{valid, message("answer", "final_answer", "")}, false},
		{"same_id_empty_phase_conflict", []protocolv2.ThreadItem{message("empty", "", ""), message("empty", "final_answer", ""), valid}, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			got, err := finalProductReply(productTurn{Items: test.items})
			if (err == nil) != test.valid || test.valid && got != "answer" {
				t.Fatalf("empty contribution changed answer or identity authority: reply=%q, error=%v", got, err)
			}
		})
	}
}

func TestFinalProductReplyUsesCompleteRelevantEvidence(t *testing.T) {
	for _, mode := range []string{"unrelated_items", "discarded_legacy", "long_final", "commentary_identity_conflict", "legacy_identity_conflict"} {
		t.Run(mode, func(t *testing.T) {
			items := []protocolv2.ThreadItem{invoiceMessage("answer", "final_answer", "answer")}
			want, valid := "answer", true
			switch mode {
			case "unrelated_items":
				for i := 0; i < 129; i++ {
					items = append(items, invoiceMessage(fmt.Sprintf("commentary-%d", i), "commentary", "progress"))
				}
			case "discarded_legacy":
				items = append([]protocolv2.ThreadItem{invoiceMessage("legacy", "", strings.Repeat("x", 5000))}, items...)
			case "long_final":
				want = strings.Repeat("长", 2000)
				items = []protocolv2.ThreadItem{invoiceMessage("head", "final_answer", want[:3000]), invoiceMessage("tail", "final_answer", want[3000:])}
				want = want[:3000] + "\n" + want[3000:]
			case "commentary_identity_conflict":
				items = append([]protocolv2.ThreadItem{invoiceMessage("answer", "commentary", "progress")}, items...)
				valid = false
			case "legacy_identity_conflict":
				items = append(items, invoiceMessage("legacy", "", "old"), invoiceMessage("legacy", "", "changed"))
				valid = false
			}
			reply, err := finalProductReply(productTurn{Items: items})
			if (err == nil) != valid || valid && reply != want {
				t.Fatalf("selected reply lost content or conflict: bytes=%d err=%v", len(reply), err)
			}
		})
	}
}
