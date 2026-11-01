package live

import (
	"encoding/json"
	"strings"
	"sync"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// Only consumed collaboration input and final-message identity/order establish
// delivery. Durable public text owns the business result. Other raw traffic is
// optional diagnostics, not a freshness protocol or an acceptance requirement.
type childStream struct {
	diagnostics *Diagnostics
	parentID    string
	mu          sync.Mutex
	events      []frame
	bytes       int
	err         error
}

const childStreamEventLimit = 512

type childProof uint8

const (
	childNotProven childProof = iota
	childPending
	childProven
)

type childRawEvent struct {
	ThreadID, TurnID string
	Item             struct {
		ID, Type, Role, Phase, Author, Recipient string
		Content                                  []struct{ Type, Text string }
	}
}

func (stream *childStream) observe(message frame) error {
	stream.recordDiagnostic(message)
	keep := message.Method == "turn/completed"
	if message.Method == "rawResponseItem/completed" {
		var event childRawEvent
		if json.Unmarshal(message.Params, &event) == nil && event.ThreadID == stream.parentID {
			keep = event.Item.Type == "agent_message" || event.Item.Type == "message" && event.Item.Role == "assistant"
			if keep && event.Item.Type == "message" {
				// Only identity, phase and first-seen timing are proof data. Raw
				// text can contain hidden markup and never owns invoice semantics.
				event.Item.Content = nil
				message.Params, _ = json.Marshal(event)
				message.size = len(message.Params)
			}
		}
	}
	if !keep {
		return nil
	}
	stream.mu.Lock()
	defer stream.mu.Unlock()
	if stream.err != nil {
		return stream.err
	}
	size := max(message.size, len(message.Params))
	if len(stream.events) >= childStreamEventLimit || size > (1<<20)-stream.bytes {
		stream.err = failedObservation("child_delivery_evidence_budget_exceeded", "live: child delivery evidence budget exceeded")
		return stream.err
	}
	message.Params = append(json.RawMessage(nil), message.Params...)
	stream.events = append(stream.events, message)
	stream.bytes += size
	return nil
}

func (stream *childStream) failure() error {
	stream.mu.Lock()
	defer stream.mu.Unlock()
	return stream.err
}
func (stream *childStream) snapshot() []frame {
	stream.mu.Lock()
	defer stream.mu.Unlock()
	return append([]frame(nil), stream.events...)
}

func (stream *childStream) terminalConflict(threadID, turnID string) bool {
	for _, message := range stream.snapshot() {
		value, ok := message.Notification.AsTurnCompleted()
		if !ok || value.Params.ThreadID != threadID || value.Params.Turn.ID != turnID {
			continue
		}
		turn, err := productTurnFromSDK(value.Params.Turn)
		if err != nil || completedProductTurn(turn) != nil {
			return true
		}
	}
	return false
}

func (stream *childStream) proof(child, parent productThread, credited, current productTurn, boundChildren map[string]bool) childProof {
	events := stream.snapshot()
	path := child.Source.SubAgent.ThreadSpawn.AgentPath
	deliveredAt := -1
	// A replay of the same pre-delivery item is not a new parent answer.
	firstSeen := make(map[string]int)
	phases := make(map[string]string)
	reject := func(reason string) childProof {
		stream.diagnostics.Record("child_proof_rejected", map[string]any{"child_id": child.ID, "turn_id": credited.ID, "reason": reason})
		return childNotProven
	}
	for index, message := range events {
		if message.Method == "turn/completed" {
			value, ok := message.Notification.AsTurnCompleted()
			if !ok {
				return reject("invalid_terminal")
			}
			relevant := value.Params.ThreadID == child.ID && value.Params.Turn.ID == credited.ID ||
				value.Params.ThreadID == parent.ID && value.Params.Turn.ID == current.ID
			if relevant {
				turn, err := productTurnFromSDK(value.Params.Turn)
				if err != nil || completedProductTurn(turn) != nil {
					return reject("terminal_contradiction")
				}
			}
			continue
		}
		var event childRawEvent
		if json.Unmarshal(message.Params, &event) != nil {
			return reject("invalid_delivery_event")
		}
		if event.ThreadID != parent.ID || event.TurnID != current.ID {
			continue
		}
		item := event.Item
		if item.Type == "agent_message" && item.Author == path && item.Recipient == "/root" {
			var parts []string
			valid := len(item.Content) != 0
			for _, part := range item.Content {
				valid = valid && part.Type == "input_text"
				parts = append(parts, part.Text)
			}
			text := strings.Join(parts, "")
			// The stock projection wraps the business result in an AgentMessage
			// header. Routing authority is the typed author/recipient above.
			if _, payload, ok := strings.Cut(text, "\nPayload:\n"); ok {
				text = payload
			}
			if valid && validChildInvoice(text) && deliveredAt < 0 {
				deliveredAt = index
			}
		}
		if item.Type == "message" && item.Role == "assistant" {
			if item.ID == "" {
				return reject("parent_message_identity_missing")
			}
			if previous, seen := phases[item.ID]; seen && previous != item.Phase {
				return reject("parent_message_identity_conflict")
			}
			if _, seen := firstSeen[item.ID]; !seen {
				firstSeen[item.ID] = index
				phases[item.ID] = item.Phase
			}
		}
	}
	if deliveredAt < 0 {
		return childPending
	}
	items, err := childTerminalItems(current, boundChildren)
	if err != nil {
		return reject("parent_terminal_invalid")
	}
	// Keep the durable terminal priority and creation order. Raw completion
	// order can differ; raw text can still contain markup stripped by stock.
	hasFinal := false
	for _, item := range items {
		if message, ok := item.AsAgentMessage(); ok && strings.TrimSpace(message.Text) != "" && message.Phase != nil && message.Phase.Value != nil && *message.Phase.Value == protocolv2.MessagePhaseFinalAnswer {
			hasFinal = true
		}
	}
	var witnessed []protocolv2.ThreadItem
	allSeen := true
	for _, item := range items {
		message, ok := item.AsAgentMessage()
		if !ok {
			continue
		}
		phase := ""
		if message.Phase != nil && message.Phase.Value != nil {
			phase = string(*message.Phase.Value)
		}
		if phase != "" && phase != "final_answer" || hasFinal && phase != "final_answer" {
			continue
		}
		at, seen := firstSeen[message.ID]
		allSeen = allSeen && seen
		if seen && at > deliveredAt {
			if phases[message.ID] != phase {
				return reject("durable_message_phase_mismatch")
			}
			witnessed = append(witnessed, item)
		}
	}
	reply, err := finalProductReply(productTurn{Items: witnessed})
	if err == nil && validChildInvoice(reply) {
		return childProven
	}
	if allSeen {
		return reject("parent_result_not_after_delivery")
	}
	// A thread/read response is not a fence for independently delivered raw
	// callbacks. Missing fragments remain pending until the original deadline.
	return childPending
}

func (stream *childStream) recordDiagnostic(message frame) {
	if message.Method != "rawResponseItem/completed" {
		return
	}
	var event childRawEvent
	if json.Unmarshal(message.Params, &event) != nil {
		return
	}
	if event.Item.Type != "agent_message" && event.Item.Type != "message" {
		return
	}
	var text []string
	for _, part := range event.Item.Content {
		if part.Type == "input_text" || part.Type == "output_text" {
			text = append(text, part.Text)
		}
	}
	stream.diagnostics.Record("child_visible_message", map[string]any{"thread_id": event.ThreadID, "turn_id": event.TurnID, "item_id": event.Item.ID, "type": event.Item.Type, "role": event.Item.Role, "phase": event.Item.Phase, "author": event.Item.Author, "recipient": event.Item.Recipient, "text": text})
}
