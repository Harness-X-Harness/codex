package live

import (
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
