package live

import (
	"crypto/sha256"
	"testing"
)

func TestHistoryFailureClassifiesObservedBoundary(t *testing.T) {
	for _, test := range []struct {
		name, reply, previous, code string
		price                       int
		accepted                    bool
	}{
		{"lookup_unobserved", `{"amount":16}`, "", "history_lookup_not_observed", 0, false},
		{"invalid_reply", `not an amount`, "", "history_reply_invalid", 0, false},
		{"first_amount_wrong", `{"amount":16}`, "", "history_amount_mismatch", 7, true},
		{"continuation_wrong", `{"amount":14}`, "first", "history_reply_not_recalled", 7, true},
	} {
		t.Run(test.name, func(t *testing.T) {
			o := newSDKHistoryObserver()
			o.unitPrice, o.previousID = test.price, test.previous
			if test.accepted {
				o.acceptedCalls[sha256.Sum256([]byte("call"))] = struct{}{}
			}
			if err := o.completedTurn(historyTurn{Status: "completed", Items: []historyItem{{Type: "agentMessage", ID: "reply", Phase: "final_answer", Text: test.reply}}}, &o.reply); err != nil {
				t.Fatal(err)
			}
			if o.reply.recall || DescribeFailure(o.replyFailure()).Code != test.code {
				t.Fatal("wrong observed-boundary classification")
			}
			if o.unitPrice != test.price || len(o.acceptedCalls) != map[bool]int{false: 0, true: 1}[test.accepted] {
				t.Fatal("diagnosis invented lookup evidence")
			}
		})
	}
}

// Model witness: inline correct -> early assertion -> delayed authoritative
// wrong final -> required facts -> settlement. Freezing a previously checked
// boolean would accept this trace. Settlement must recheck and freeze together.
func TestHistorySettlementRechecksInlineReply(t *testing.T) {
	o := newSDKHistoryObserver()
	o.active, o.threadID, o.turnID, o.reconciled = true, "thread", "turn", true
	o.unitPrice = 7
	o.acceptedCalls[sha256.Sum256([]byte("call"))] = struct{}{}
	if err := o.completedTurn(historyTurn{Status: "completed", Items: []historyItem{{Type: "agentMessage", ID: "inline", Phase: "final_answer", Text: `{"amount":14}`}}}, &o.reply); err != nil {
		t.Fatal(err)
	}
	if err := o.assertAmount(2); err != nil {
		t.Fatal("initial response not accepted")
	}
	// Callback arrival replaces the selected response before the fact boundary.
	if err := o.accept(sdkHistoryObservation{Method: "turn/completed", ThreadID: "thread", Turn: historyTurn{ID: "turn", Status: "completed", Items: []historyItem{{Type: "agentMessage", ID: "terminal", Phase: "final_answer", Text: `{"amount":21}`}}}}); err != nil {
		t.Fatal(err)
	}
	o.observations.ReasoningItems, o.observations.EncryptedItems, o.observations.CompletedTools = 1, 1, 1
	if err := o.settleTurn(2, nil); DescribeFailure(err).Code != "history_amount_mismatch" {
		t.Fatal("stale business assertion authorized continuation")
	}
	if o.active || o.observations.FirstRecall || !o.observations.FirstCompleted {
		t.Fatal("settlement did not freeze truthful partial evidence")
	}
}

func TestHistorySettlementCannotGainLateCredit(t *testing.T) {
	o := newSDKHistoryObserver()
	o.active, o.threadID, o.turnID, o.reconciled = true, "thread", "turn", true
	o.unitPrice = 7
	prior := failedObservation("deadline_exceeded", "live: deadline exceeded")
	if o.settleTurn(2, prior) != prior {
		t.Fatal("partial arithmetic replaced the original failure")
	}
	if err := o.observe(sdkHistoryObservation{Method: "turn/completed", ThreadID: "thread", Turn: historyTurn{ID: "turn", Status: "completed", Items: []historyItem{{Type: "agentMessage", ID: "late", Text: `{"amount":14}`}}}}, 1, nil); err != nil {
		t.Fatal(err)
	}
	if o.observations.FirstCompleted || o.observations.FirstRecall {
		t.Fatal("late callback gained positive proof credit")
	}
}

func TestHistorySettlementRejectsConflictAfterCollection(t *testing.T) {
	o := newSDKHistoryObserver()
	o.active, o.threadID, o.turnID, o.reconciled = true, "thread", "turn", true
	o.unitPrice = 7
	o.reply.completed = true
	o.completedItem(historyItem{Type: "agentMessage", ID: "reply", Phase: "final_answer", Text: `{"amount":14}`}, &o.reply)
	if err := o.assertAmount(2); err != nil {
		t.Fatal(err)
	}
	// Evidence is still open while the required raw/tool facts arrive.
	o.completedItem(historyItem{Type: "agentMessage", ID: "reply", Phase: "final_answer", Text: `{"amount":21}`}, &o.reply)
	o.observations.ReasoningItems, o.observations.EncryptedItems, o.observations.CompletedTools = 1, 1, 1
	if err := o.settleTurn(2, nil); err == nil || o.observations.FirstRecall {
		t.Fatal("conflicting callback authorized continuation")
	}
}
