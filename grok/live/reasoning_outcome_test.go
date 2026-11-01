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
			o.probe.unitPrice, o.probe.previousID = test.price, test.previous
			if test.accepted {
				o.acceptedCalls[sha256.Sum256([]byte("call"))] = struct{}{}
			}
			if err := o.probe.completedTurn(historyTurn{Status: "completed", Items: []historyItem{{Type: "agentMessage", ID: "reply", Phase: "final_answer", Text: test.reply}}}, &o.reply); err != nil {
				t.Fatal(err)
			}
			if o.reply.recall || DescribeFailure(o.replyFailure()).Code != test.code {
				t.Fatal("wrong observed-boundary classification")
			}
			if o.probe.unitPrice != test.price || len(o.acceptedCalls) != map[bool]int{false: 0, true: 1}[test.accepted] {
				t.Fatal("diagnosis invented lookup evidence")
			}
		})
	}
}
