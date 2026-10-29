package live

import (
	"encoding/json"
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

func TestChildStreamAssistantTextContributions(t *testing.T) {
	const nonce = "12345678-1234-4234-8234-123456789abc"
	const marker = "controlled-marker"
	type fragment struct{ kind, text string }
	type message struct {
		id, phase string
		content   []fragment
	}
	legacy := message{"answer", "", []fragment{{"output_text", marker + " " + nonce}}}
	empty := message{"empty", "final_answer", []fragment{{"output_text", " \n "}}}
	for _, test := range []struct {
		name     string
		messages []message
		valid    bool
	}{
		{"legacy_before_empty_final", []message{legacy, empty}, true},
		{"empty_final_before_legacy", []message{empty, legacy}, true},
		{"all_empty", []message{empty}, false},
		{"missing_id", []message{{"", "final_answer", empty.content}, legacy}, false},
		{"same_id_empty_content_conflict", []message{{"answer", "", empty.content}, legacy}, false},
		{"same_id_empty_phase_conflict", []message{{"empty", "", empty.content}, empty, legacy}, false},
		{"later_nonempty_wrong_final", []message{legacy, {"wrong", "final_answer", []fragment{{"output_text", "wrong"}}}}, false},
		{"fragmented_marker", []message{{"answer", "final_answer", []fragment{{"output_text", "controlled-"}, {"output_text", "marker " + nonce}}}}, true},
		{"actual_newline_in_marker", []message{{"answer", "final_answer", []fragment{{"output_text", "controlled-\n"}, {"output_text", "marker " + nonce}}}}, false},
		{"legacy_input_text", []message{{"answer", "final_answer", []fragment{{"input_text", marker + " " + nonce}}}}, true},
		{"input_text_wrong_content", []message{{"answer", "final_answer", []fragment{{"input_text", "wrong"}}}}, false},
		{"input_text_missing_id", []message{{"", "final_answer", []fragment{{"input_text", marker + " " + nonce}}}}, false},
		{"nontext_kind", []message{{"answer", "final_answer", []fragment{{"encrypted_content", marker + " " + nonce}}}}, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			turn := productTurn{ID: "turn", Status: "completed"}
			child := productThread{ID: "child", Turns: []productTurn{turn}}
			child.Source.SubAgent.ThreadSpawn.AgentPath = "/root/child"
			stream := childStream{}
			if err := stream.observe(frame{Method: "turn/started", Notification: protocolv2.NewServerNotificationTurnStarted(protocolv2.ServerNotificationTurnStarted{Params: protocolv2.TurnStartedNotification{
				ThreadID: child.ID, Turn: protocolv2.Turn{ID: turn.ID, Status: protocolv2.TurnStatusInProgress},
			}})}); err != nil {
				t.Fatal(err)
			}
			raw := func(item map[string]any) {
				params, err := json.Marshal(map[string]any{"threadId": child.ID, "turnId": turn.ID, "item": item})
				if err != nil {
					t.Fatal(err)
				}
				if err := stream.observe(frame{Method: "rawResponseItem/completed", Params: params}); err != nil {
					t.Fatal(err)
				}
			}
			raw(map[string]any{"type": "agent_message", "id": "task", "author": "/root", "recipient": "/root/child", "content": []any{map[string]any{"type": "input_text", "text": "Return the inherited marker and a fresh result"}}})
			for _, message := range test.messages {
				var content []any
				for _, part := range message.content {
					content = append(content, map[string]any{"type": part.kind, "text": part.text})
				}
				raw(map[string]any{"type": "message", "id": message.id, "role": "assistant", "phase": message.phase, "content": content})
			}
			if err := stream.observe(frame{Method: "turn/completed", Notification: protocolv2.NewServerNotificationTurnCompleted(protocolv2.ServerNotificationTurnCompleted{Params: protocolv2.TurnCompletedNotification{
				ThreadID: child.ID, Turn: protocolv2.Turn{ID: turn.ID, Status: protocolv2.TurnStatusCompleted},
			}})}); err != nil {
				t.Fatal(err)
			}
			if got := stream.proof(child, productThread{}, turn, nonce, marker); (got == childProven) != test.valid {
				t.Fatalf("assistant projection changed causal result: got=%v want_proven=%v", got, test.valid)
			}
		})
	}
}

func TestFinalProductReplyBudgetClassification(t *testing.T) {
	for _, size := range []int{4096, 4097, 19970} {
		text := strings.Repeat("x", size)
		got, err := finalProductReply(productTurn{Items: []protocolv2.ThreadItem{
			protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "answer", Text: text, Phase: protocolv2.Value(protocolv2.MessagePhaseFinalAnswer)}),
		}})
		if size == 4096 {
			if err != nil || got != text {
				t.Fatal("existing reply boundary changed")
			}
			continue
		}
		if err == nil || got != "" || DescribeFailure(err).Code != "product_reply_budget_exceeded" {
			t.Fatalf("oversized reply lost typed rejection: %v", err)
		}
		encoded, _ := json.Marshal(DescribeFailure(err))
		if strings.Contains(string(encoded), text) {
			t.Fatal("failure exported answer content")
		}
	}
}
