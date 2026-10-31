package live

import (
	"errors"
	"testing"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func TestBasicJoinedNotificationsPreserveBoundProof(t *testing.T) {
	for _, test := range []struct {
		name, thread, turn string
		status             protocolv2.TurnStatus
		rejected           bool
	}{
		{"matching_failed", "thread", "turn", protocolv2.TurnStatusFailed, true},
		{"matching_interrupted", "thread", "turn", protocolv2.TurnStatusInterrupted, true},
		{"unrelated_thread", "other", "turn", protocolv2.TurnStatusFailed, false},
		{"unrelated_turn", "thread", "other", protocolv2.TurnStatusFailed, false},
		{"matching_completed", "thread", "turn", protocolv2.TurnStatusCompleted, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			evidence := &Evidence{Completed: true, ReplyBytes: 5, Stage: "final_reply"}
			queue := make(chan protocolv2.ServerNotification, 1)
			queue <- protocolv2.NewServerNotificationTurnCompleted(protocolv2.ServerNotificationTurnCompleted{
				Params: protocolv2.TurnCompletedNotification{ThreadID: test.thread, Turn: protocolv2.Turn{ID: test.turn, Status: test.status}},
			})
			observer := &basicObservation{threadID: "thread", turnID: "turn", evidence: evidence, notifications: queue}
			err := observer.afterClose("", evidence, nil)
			if (err != nil) != test.rejected || evidence.Completed == test.rejected {
				t.Fatalf("joined bound terminal proof mismatch: %+v, %v", evidence, err)
			}
			if test.rejected && evidence.Stage == "final_reply" || !test.rejected && evidence.Stage != "final_reply" {
				t.Fatal("final reply stage contradicts completion")
			}
		})
	}
}

func TestBasicCleanupCannotCreateCompletion(t *testing.T) {
	evidence := &Evidence{Stage: "turn_submitted"}
	queue := make(chan protocolv2.ServerNotification, 1)
	queue <- protocolv2.NewServerNotificationTurnCompleted(protocolv2.ServerNotificationTurnCompleted{
		Params: protocolv2.TurnCompletedNotification{ThreadID: "thread", Turn: protocolv2.Turn{ID: "turn", Status: protocolv2.TurnStatusCompleted}},
	})
	observer := &basicObservation{threadID: "thread", turnID: "turn", evidence: evidence, notifications: queue}
	if err := observer.afterClose("", evidence, nil); err != nil || evidence.Completed || evidence.Stage != "turn_submitted" {
		t.Fatal("cleanup created missing business proof")
	}
}

func TestBasicRejectedRunRetainsPartialTerminalFacts(t *testing.T) {
	// A completed turn with no eligible reply is still a rejected scenario;
	// its diagnostic terminal fact is not overall success credit.
	evidence := &Evidence{Completed: true, ReplyBytes: 0, Stage: "turn_completed"}
	observer := &basicObservation{evidence: evidence}
	if err := observer.afterClose("", evidence, errors.New("missing reply")); err != nil || !evidence.Completed || evidence.Stage != "turn_completed" {
		t.Fatal("cleanup erased a rejected run's partial terminal facts")
	}
}
