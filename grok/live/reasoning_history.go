package live

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"strings"
)

const historyTool = "grok_history_probe"

// This bounds replay evidence, not product tool calls or retries.
const historyAssistantLimit = 4096

// ReasoningHistory observes two same-thread turns without semantic reinvocation.
// It leaves reasoning effort to the configured model rather than overriding it.
// Native encrypted replay/tool eligibility and Subject provenance are prerequisites;
// deterministic fixtures do not establish real-provider Story success.
func ReasoningHistory(ctx context.Context, options Options) (Evidence, error) {
	return sdkReasoningHistory(ctx, options)
}

// Only the token, current turn identities and bounded reply evidence survive frames.
// Repeated internal calls are counted, never accumulated in a call-ID/event map.
type historyProbe struct {
	evidence                     *Evidence
	threadID, turnID, previousID string
	token                        string
}

type historyItem struct {
	ID               string  `json:"id"`
	Type             string  `json:"type"`
	Phase            string  `json:"phase"`
	Text             string  `json:"text"`
	Namespace        *string `json:"namespace"`
	Tool             string  `json:"tool"`
	Status           string  `json:"status"`
	Success          bool    `json:"success"`
	ContentItems     []item  `json:"contentItems"`
	EncryptedContent string  `json:"encrypted_content"`
}

type historyTurn struct {
	ID     string          `json:"id"`
	Status string          `json:"status"`
	Error  json.RawMessage `json:"error"`
	Items  []historyItem   `json:"items"`
}

type historyReply struct {
	bytes                                       int
	completed, recall, authoritative, ambiguous bool
	identities                                  map[[sha256.Size]byte]historyReplyOutcome
}

type historyReplyOutcome struct {
	finalEligible, recall bool
}

func (probe *historyProbe) completedTurn(turn historyTurn, reply *historyReply) error {
	if turn.Status != "completed" || len(turn.Error) != 0 && string(turn.Error) != "null" {
		return failedTurn(turn.Status, turn.Error, "live: turn did not complete successfully")
	}
	reply.completed = true
	// A terminal summary's last assistant message is authoritative, even if an
	// earlier item recalled the token or an older notification is replayed later.
	for _, item := range turn.Items {
		if item.Type == "agentMessage" {
			if item.ID == "" {
				return failedObservation("assistant_identity_missing", "live: assistant item identity not established")
			}
			// Summary order overrides deduplication without spending its budget.
			reply.bytes = len(item.Text)
			reply.recall = probe.token != "" && (item.Phase == "" || item.Phase == "final_answer") && strings.Contains(item.Text, probe.token)
			reply.authoritative, reply.ambiguous = true, false
			reply.identities = nil
		} else if err := probe.completedItem(item, reply); err != nil {
			return err
		}
	}
	return nil
}

func (probe *historyProbe) completedItem(item historyItem, reply *historyReply) error {
	if item.Type == "agentMessage" {
		if item.ID == "" {
			return failedObservation("assistant_identity_missing", "live: assistant item identity not established")
		}
		if reply.authoritative {
			return nil
		}
		identity := sha256.Sum256([]byte(item.ID))
		outcome := historyReplyOutcome{
			finalEligible: item.Phase == "" || item.Phase == "final_answer",
			recall:        probe.token != "" && strings.Contains(item.Text, probe.token),
		}
		if previous, seen := reply.identities[identity]; seen {
			if previous != outcome {
				reply.ambiguous = true
			}
			return nil
		}
		if len(reply.identities) == historyAssistantLimit {
			return failedObservation("assistant_identity_budget_exceeded", "live: assistant identity evidence budget exceeded")
		}
		reply.identities[identity] = outcome
		// Unknown phase remains provisional. Only an unseen item can supersede
		// it; replay cannot resurrect an old candidate. A summary always wins.
		reply.bytes, reply.recall = len(item.Text), outcome.finalEligible && outcome.recall
	}
	if probe.previousID != "" {
		return nil
	}
	if item.Type == "reasoning" {
		probe.evidence.ReasoningItems++
	}
	if item.Type == "dynamicToolCall" && item.ID != "" && item.Namespace == nil && item.Tool == historyTool && item.Status == "completed" && item.Success && probe.token != "" {
		for _, content := range item.ContentItems {
			if content.Type == "inputText" && strings.Contains(content.Text, probe.token) {
				probe.evidence.CompletedTools++
				break
			}
		}
	}
	return nil
}
