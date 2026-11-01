package live

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"strings"
)

const historyTool = "apple_price_lookup"

const historyOutputSchema = `{"type":"object","properties":{"amount":{"type":["number","null"]}},"required":["amount"],"additionalProperties":false}`

// This bounds replay evidence, not product tool calls or retries.
const historyAssistantLimit = 4096

// ReasoningHistory observes two same-thread turns without semantic reinvocation.
// It leaves reasoning effort to the configured model rather than overriding it.
// Native encrypted replay/tool eligibility and Subject provenance are prerequisites;
// deterministic fixtures do not establish real-provider Story success.
func ReasoningHistory(ctx context.Context, options Options) (Evidence, error) {
	return sdkReasoningHistory(ctx, options)
}

// Only the unit price and tool result, current turn identities and bounded reply evidence survive frames.
// Repeated internal calls are counted, never accumulated in a call-ID/event map.
type historyProbe struct {
	diagnostics                  *Diagnostics
	evidence                     *Evidence
	threadID, turnID, previousID string
	unitPrice                    int
	toolResult                   string
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
	text                                        strings.Builder
	formatValid, evaluated                      bool
	completed, recall, authoritative, ambiguous bool
	explicitFinal                               bool
	identities                                  map[[sha256.Size]byte]historyReplyOutcome
}

type historyReplyOutcome struct {
	finalEligible, explicitFinal, hasContent bool
	textSHA                                  [sha256.Size]byte
}

func (probe *historyProbe) completedTurn(turn historyTurn, reply *historyReply) error {
	if turn.Status != "completed" || len(turn.Error) != 0 && string(turn.Error) != "null" {
		return failedTurn(turn.Status, turn.Error, "live: turn did not complete successfully")
	}
	reply.completed = true
	// The public MessagePhase owner distinguishes terminal answers from
	// commentary/partial text. An authoritative summary combines all explicit
	// final answers, with phase-unknown legacy fallback only when no final exists.
	summaryAssistant := false
	summaryIdentities := make(map[[sha256.Size]byte]historyReplyOutcome)
	for _, item := range turn.Items {
		if item.Type == "agentMessage" {
			if item.ID == "" {
				return failedObservation("assistant_identity_missing", "live: assistant item identity not established")
			}
			if !summaryAssistant {
				// A nonempty assistant summary supersedes notifications even when
				// it has no eligible result; it cannot borrow an older answer.
				probe.diagnostics.Record("oracle_reply_reset", map[string]any{"scenario": "reasoning", "turn_id": probe.turnID, "source": "terminal_items"})
				reply.bytes, reply.recall, reply.explicitFinal = 0, false, false
				reply.text.Reset()
				reply.formatValid, reply.evaluated = false, false
				reply.authoritative, reply.ambiguous = true, false
				reply.identities = nil
				summaryAssistant = true
			}
			identity := sha256.Sum256([]byte(item.ID))
			outcome := historyItemOutcome(item)
			if previous, seen := summaryIdentities[identity]; seen {
				if previous != outcome {
					reply.ambiguous = true
					return failedObservation("assistant_identity_conflict", "live: authoritative reply identity conflicted")
				}
				continue
			}
			if len(summaryIdentities) == historyAssistantLimit {
				return failedObservation("assistant_identity_budget_exceeded", "live: assistant identity evidence budget exceeded")
			}
			summaryIdentities[identity] = outcome
			if strings.TrimSpace(item.Text) == "" {
				continue
			}
			if item.Phase == "final_answer" || item.Phase == "" && !reply.explicitFinal {
				probe.addReplyFragment(item, reply)
			}
		} else if err := probe.completedItem(item, reply); err != nil {
			return err
		}
	}
	probe.evaluateReply(reply)
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
		outcome := historyItemOutcome(item)
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
		// Empty finalized text contributes no candidate, but still carries identity.
		if !outcome.hasContent {
			return nil
		}
		// Combine unseen eligible fragments from the entire final response.
		// Explicit finals outrank legacy text; commentary never earns recall.
		// Replay and conflicting identities stay above.
		if item.Phase == "final_answer" || item.Phase == "" && !reply.explicitFinal {
			probe.addReplyFragment(item, reply)
		}
	}
	if probe.previousID != "" {
		return nil
	}
	if item.Type == "reasoning" {
		probe.evidence.ReasoningItems++
	}
	if item.Type == "dynamicToolCall" && item.ID != "" && item.Namespace == nil && item.Tool == historyTool && item.Status == "completed" && item.Success && probe.unitPrice > 0 && probe.toolResult != "" {
		for _, content := range item.ContentItems {
			if content.Type == "inputText" && strings.Contains(content.Text, probe.toolResult) {
				probe.evidence.CompletedTools++
				break
			}
		}
	}
	return nil
}

// All eligible final fragments form one structured result; never pick a valid
// object from a larger malformed answer. Final phase still outranks legacy text.
func (probe *historyProbe) addReplyFragment(item historyItem, reply *historyReply) {
	probe.diagnostics.Record("oracle_reply_fragment", map[string]any{"scenario": "reasoning", "turn_id": probe.turnID, "item_id": item.ID, "phase": item.Phase, "text": item.Text, "reset_legacy": item.Phase == "final_answer" && !reply.explicitFinal, "authoritative": reply.authoritative})
	if item.Phase == "final_answer" && !reply.explicitFinal {
		reply.bytes = 0
		reply.text.Reset()
	}
	reply.bytes += len(item.Text)
	if reply.text.Len() != 0 {
		reply.text.WriteByte('\n')
	}
	reply.text.WriteString(item.Text)
	reply.formatValid, reply.recall, reply.evaluated = false, false, false
	reply.explicitFinal = item.Phase == "final_answer"
}

// SDK retention and frame limits own resource admission. Assemble the complete
// selected answer without repeatedly copying or parsing its growing prefixes.
// Turn completion and final observation boundaries request evaluation; an
// unchanged collected reply reuses it, while new fragments cannot borrow recall.
func (probe *historyProbe) evaluateReply(reply *historyReply) {
	if reply.evaluated {
		return
	}
	var result struct {
		Amount *float64 `json:"amount"`
	}
	err := parseStructuredReply(reply.text.String(), &result, "amount")
	reply.formatValid = err == nil
	quantity := 2
	if probe.previousID != "" {
		quantity = 3
	}
	reply.recall = err == nil && result.Amount != nil && probe.unitPrice > 0 && *result.Amount == float64(quantity*probe.unitPrice)
	reply.evaluated = true
}

func historyItemOutcome(item historyItem) historyReplyOutcome {
	return historyReplyOutcome{finalEligible: item.Phase == "" || item.Phase == "final_answer", explicitFinal: item.Phase == "final_answer", hasContent: strings.TrimSpace(item.Text) != "", textSHA: sha256.Sum256([]byte(item.Text))}
}
