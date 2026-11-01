package live

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strconv"
	"strings"
	"testing"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

const testInvoice = "item,quantity,unit_price,line_total\nnotebook,3,4.50,13.50\npen,2,1.25,2.50\ntotal,,,16.00"

func TestChildInvoice(t *testing.T) {
	for _, tc := range []struct {
		name, text string
		valid      bool
	}{
		{"plain", testInvoice, true},
		{"large_fenced_prose", strings.Repeat("Checked.\n", 1200) + "```csv\n" + testInvoice + "\n```\n" + strings.Repeat("Thanks.\n", 1200), true},
		{"long_equivalent_decimals", strings.ReplaceAll(strings.ReplaceAll(testInvoice, "4.50", "00000000000004.500000000000"), "1.25", "000000000001.250000000000"), true},
		{"long_prose_after_fence", "```csv\n" + testInvoice + "\n```\nThanks.\nOrder checked.\nNo tax.\nNo shipping.\nReady to import.", true},
		{"crlf", strings.ReplaceAll(testInvoice, "\n", "\r\n"), true},
		{"fenced_prose", "Here is the checked invoice.\n```csv\n" + testInvoice + "\n```\nTotal includes no tax.", true},
		{"reordered", "line_total,item,unit_price,quantity\n16,total,,\n2.5,pen,1.2500,2\n13.500,notebook,4.5,3", true},
		{"quoted", "\"item\",quantity,unit_price,line_total\n\"notebook\",3,4.50,13.50\npen,2,1.25,2.50\ntotal,,,16.00", true},
		{"decimal_quantities", strings.Replace(strings.Replace(testInvoice, "notebook,3,", "notebook,3.0,", 1), "pen,2,", "pen,2.00,", 1), true},
		{"fractional_quantity", strings.Replace(testInvoice, "notebook,3,", "notebook,3.1,", 1), false},
		{"wrong_total", strings.Replace(testInvoice, "16.00", "15.00", 1), false},
		{"wrong_line", strings.Replace(testInvoice, "13.50", "12.00", 1), false},
		{"missing_row", strings.Replace(testInvoice, "pen,2,1.25,2.50\n", "", 1), false},
		{"duplicate_row", testInvoice + "\npen,2,1.25,2.50", false},
		{"duplicate_column", strings.Replace(testInvoice, "unit_price", "quantity", 1), false},
		{"extra_column", strings.Replace(testInvoice, "item,", "other,item,", 1), false},
		{"extra_data", testInvoice + "\nignore", false},
		{"two_bare", testInvoice + "\n" + testInvoice, false},
		{"two_fenced", "```csv\n" + testInvoice + "\n```\n```csv\n" + testInvoice + "\n```", false},
		{"fenced_and_bom_bare", "```csv\n" + testInvoice + "\n```\n\ufeff" + testInvoice, false},
		{"fenced_and_quoted_multiline_header", "```csv\n" + testInvoice + "\n```\n\"item\n\",quantity,unit_price,line_total\npen,2,1.25,2.50", false},
		{"fenced_and_unicode_case_header", "```csv\n" + testInvoice + "\n```\n" + strings.Replace(testInvoice, "item", "İTEM", 1), false},
		{"fenced_and_bom_whitespace_header", "```csv\n" + testInvoice + "\n```\n" + strings.Replace(testInvoice, "item,quantity", "item,\ufeff \u2003quantity", 1), false},
		{"fenced_and_quoted_bom_whitespace_header", "```csv\n" + testInvoice + "\n```\n" + strings.Replace(testInvoice, "item,quantity", "item,\"\ufeff \u2003quantity\"", 1), false},
		{"malformed_quote_before_second_header", "```csv\n" + testInvoice + "\n```\n\"unterminated prose\n\"item\n\",quantity,unit_price,line_total\npen,2,1.25,2.50", false},
		{"fractional_cent", strings.Replace(testInvoice, "4.50", "4.501", 1), false},
		{"nonfinite", strings.Replace(testInvoice, "4.50", "NaN", 1), false},
		{"ready", "The invoice is ready.", false},
		{"empty", "", false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if got := validChildInvoice(tc.text); got != tc.valid {
				t.Fatalf("invoice=%t, want %t", got, tc.valid)
			}
		})
	}
}

func invoiceMessage(id, phase, text string) protocolv2.ThreadItem {
	item := protocolv2.ThreadItemAgentMessage{ID: id, Text: text}
	if phase != "" {
		item.Phase = protocolv2.Value(protocolv2.MessagePhase(phase))
	}
	return protocolv2.NewThreadItemAgentMessage(item)
}
func rawInvoice(t *testing.T, thread, turn string, item map[string]any) frame {
	t.Helper()
	data, err := json.Marshal(map[string]any{"threadId": thread, "turnId": turn, "item": item})
	if err != nil {
		t.Fatal(err)
	}
	return frame{Method: "rawResponseItem/completed", Params: data, size: len(data)}
}
func TestChildInvoiceDelivery(t *testing.T) {
	for _, mode := range []string{"ok", "raw_text_not_authority", "missing_delivery", "wrong_author", "wrong_recipient", "wrong_invoice", "wrong_parent_turn", "pre_delivery_replay", "phase_identity_reuse", "late_callback", "durable_order", "phase_less_tool_round", "terminal_conflict", "irrelevant_prose_before_delivery"} {
		t.Run(mode, func(t *testing.T) {
			child := productThread{ID: "child"}
			child.Source.SubAgent.ThreadSpawn.AgentPath = "/root/checker"
			parent := productThread{ID: "parent"}
			own := productTurn{ID: "child-turn", Status: "completed", Items: []protocolv2.ThreadItem{invoiceMessage("child-final", "", testInvoice)}}
			current := productTurn{ID: "parent-turn", Status: "completed", Items: []protocolv2.ThreadItem{invoiceMessage("answer", "", testInvoice)}}
			stream := childStream{parentID: "parent"}
			observe := func(item map[string]any) {
				t.Helper()
				thread, turn := "parent", "parent-turn"
				if mode == "wrong_parent_turn" {
					turn = "other"
				}
				if err := stream.observe(rawInvoice(t, thread, turn, item)); err != nil {
					t.Fatal(err)
				}
			}
			final := map[string]any{"type": "message", "id": "answer", "role": "assistant", "phase": "", "content": []any{map[string]any{"type": "output_text", "text": testInvoice}}}
			if mode == "raw_text_not_authority" {
				final["content"] = []any{map[string]any{"type": "output_text", "text": "hidden markup around raw content"}}
			}
			if mode == "phase_identity_reuse" {
				observe(map[string]any{"type": "message", "id": "answer", "role": "assistant", "phase": "commentary", "content": []any{map[string]any{"type": "output_text", "text": "Preparing."}}})
			}
			if mode == "pre_delivery_replay" {
				observe(final)
			}
			if mode == "irrelevant_prose_before_delivery" {
				current.Items = append([]protocolv2.ThreadItem{invoiceMessage("prose", "", "Here is the invoice:")}, current.Items...)
				final["content"] = []any{map[string]any{"type": "output_text", "text": "```csv\n" + testInvoice + "\n```"}}
				current.Items[1] = invoiceMessage("answer", "", "```csv\n"+testInvoice+"\n```")
				observe(map[string]any{"type": "message", "id": "prose", "role": "assistant", "content": []any{map[string]any{"type": "output_text", "text": "Here is the invoice:"}}})
			}
			delivery := map[string]any{"type": "agent_message", "author": "/root/checker", "recipient": "/root", "content": []any{map[string]any{"type": "input_text", "text": "Message Type: FINAL_ANSWER\nTask name: /root\nSender: /root/checker\nPayload:\n" + testInvoice}}}
			if mode == "wrong_author" {
				delivery["author"] = "/root/other"
			}
			if mode == "wrong_recipient" {
				delivery["recipient"] = "/root/other"
			}
			if mode == "wrong_invoice" {
				delivery["content"] = []any{map[string]any{"type": "input_text", "text": "ready"}}
			}
			if mode != "missing_delivery" {
				observe(delivery)
			}
			if mode == "durable_order" || mode == "late_callback" {
				lines := strings.SplitN(testInvoice, "\n", 3)
				current.Items = []protocolv2.ThreadItem{invoiceMessage("head", "", strings.Join(lines[:2], "\n")), invoiceMessage("tail", "", lines[2])}
				observe(map[string]any{"type": "message", "id": "tail", "role": "assistant", "content": []any{map[string]any{"type": "output_text", "text": lines[2]}}})
				if got := stream.proof(child, parent, own, current, nil); got != childPending {
					t.Fatalf("incomplete raw callback treated as fence: %v", got)
				}
				observe(map[string]any{"type": "message", "id": "head", "role": "assistant", "content": []any{map[string]any{"type": "output_text", "text": strings.Join(lines[:2], "\n")}}})
			} else {
				observe(final)
			}
			if mode == "phase_less_tool_round" {
				var tool protocolv2.ThreadItem
				if err := json.Unmarshal([]byte(`{"type":"functionCallOutput","id":"tool-result","name":"check","output":"done"}`), &tool); err != nil {
					t.Fatal(err)
				}
				current.Items = append([]protocolv2.ThreadItem{invoiceMessage("before", "", "I am checking."), tool}, current.Items...)
			}
			if mode == "terminal_conflict" {
				if err := stream.observe(frame{Method: "turn/completed", Notification: protocolv2.NewServerNotificationTurnCompleted(protocolv2.ServerNotificationTurnCompleted{Params: protocolv2.TurnCompletedNotification{ThreadID: "child", Turn: protocolv2.Turn{ID: "child-turn", Status: protocolv2.TurnStatusFailed}}})}); err != nil {
					t.Fatal(err)
				}
			}
			got := stream.proof(child, parent, own, current, nil)
			want := childProven
			switch mode {
			case "missing_delivery", "wrong_author", "wrong_recipient", "wrong_invoice", "wrong_parent_turn":
				want = childPending
			case "pre_delivery_replay", "phase_identity_reuse", "terminal_conflict":
				want = childNotProven
			}
			if got != want {
				t.Fatalf("proof=%v want=%v", got, want)
			}
		})
	}
}

func TestChildOptionalRawNoiseDoesNotConsumeProofBudget(t *testing.T) {
	stream := childStream{parentID: "parent"}
	for i := 0; i < 600; i++ {
		if err := stream.observe(rawInvoice(t, "unrelated", "turn", map[string]any{"type": "function_call", "id": "noise", "call_id": "noise", "name": "diagnostic_only", "arguments": "{}", "status": "completed"})); err != nil {
			t.Fatal(err)
		}
	}
	for i := 0; i < 600; i++ {
		for _, thread := range []string{"child", "unrelated"} {
			if err := stream.observe(rawInvoice(t, thread, "turn", map[string]any{"type": "message", "id": "noise", "role": "assistant", "content": []any{map[string]any{"type": "output_text", "text": "Not a parent delivery witness."}}})); err != nil {
				t.Fatal(err)
			}
		}
	}
	if len(stream.snapshot()) != 0 || stream.failure() != nil {
		t.Fatal("diagnostic-only noise consumed proof budget")
	}
}

func TestChildPartialFactsAfterClose(t *testing.T) {
	for _, contradiction := range []bool{false, true} {
		evidence := Evidence{Completed: true, ChildBound: true, ChildCompleted: true, ChildResultDelivered: true}
		observer := sdkProductObserver{ctx: context.Background()}
		if contradiction {
			observer.cause = errors.New("callback contradiction")
		}
		_ = observer.afterClose("", &evidence, errors.New("invoice invalid"))
		if evidence.Completed || evidence.ChildResultDelivered || !evidence.ChildBound || evidence.ChildCompleted == contradiction {
			t.Fatalf("partial facts changed: %+v", evidence)
		}
	}
}

func TestInvoiceCentsExactRepresentations(t *testing.T) {
	maxInt := int(^uint(0) >> 1)
	for _, tc := range []struct {
		text  string
		want  int
		valid bool
	}{
		{"0000000000000000004.500000000000000000", 450, true},
		{"0.00000000000000000", 0, true},
		{"4.500000000000000001", 0, false},
		{"-1.00", 0, false},
		{"NaN", 0, false},
		{"1.2.3", 0, false},
		{fmt.Sprintf("%d.%02d", maxInt/100, maxInt%100), maxInt, true},
		{strconv.Itoa(maxInt/100 + 1), 0, false},
		{strconv.Itoa(maxInt), 0, false},
		{strings.Repeat("9", 80), 0, false},
	} {
		got, valid := invoiceCents(tc.text)
		if valid != tc.valid || valid && got != tc.want {
			t.Fatalf("invoiceCents(%q)=(%d,%t), want (%d,%t)", tc.text, got, valid, tc.want, tc.valid)
		}
	}
}

func TestInvoiceHeaderMalformedTails(t *testing.T) {
	// Every line begins an unclosed quoted field. Scanning every suffix would
	// repeatedly read the same tail; none is a second four-column report.
	prose := strings.Repeat("\"item\n", 20000)
	if !validChildInvoice(prose + "\n```csv\n" + testInvoice + "\n```") {
		t.Fatal("unrelated malformed prose changed the sole fenced invoice")
	}
	if validChildInvoice(prose + "\n```csv\n" + testInvoice + "\n```\n" + testInvoice) {
		t.Fatal("malformed prose concealed a second invoice")
	}
}
