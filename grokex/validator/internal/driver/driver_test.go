package driver

import (
	"context"
	"errors"
	"testing"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func agentMessage(id, text string) protocolv2.ThreadItem {
	return protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: id, Text: text})
}

func TestLastAgentMessageIgnoresPhaseAndEmptyItems(t *testing.T) {
	items := []protocolv2.ThreadItem{
		agentMessage("m1", "first"),
		protocolv2.NewThreadItemCommandExecution(protocolv2.ThreadItemCommandExecution{ID: "c1"}),
		agentMessage("m2", "3f2b1c9e-8d4a-4b6f-9a1e-7c5d2e8f0a11"),
		agentMessage("m3", ""),
	}
	if got := lastAgentMessage(items); got != "3f2b1c9e-8d4a-4b6f-9a1e-7c5d2e8f0a11" {
		t.Fatalf("lastAgentMessage = %q", got)
	}
	if got := lastAgentMessage(nil); got != "" {
		t.Fatalf("lastAgentMessage(nil) = %q", got)
	}
}

func TestClassifyTreatsPhaselessCompletedTurnAsCompleted(t *testing.T) {
	completed := TurnRun{Status: string(protocolv2.TurnStatusCompleted)}
	phaseless := errors.New("codexsdk: turn completed without final_answer agent message")
	if err := classify(completed, phaseless); err != nil {
		t.Fatalf("phaseless completed turn classified as %v", err)
	}
	if err := classify(TurnRun{Status: "inProgress"}, phaseless); err == nil {
		t.Fatal("phaseless error on a non-completed turn must stay an error")
	}
	if err := classify(TurnRun{}, context.DeadlineExceeded); !errors.Is(err, ErrDeadline) {
		t.Fatalf("deadline classified as %v", err)
	}
	if err := classify(completed, nil); err != nil {
		t.Fatalf("nil classified as %v", err)
	}
}
