package live

import (
	"encoding/json"
	"errors"
	"reflect"
	"strings"
)

// childStream is private to the shipped child scenario. The public raw opt-in
// carries consumed AgentMessage inputs that durable Thread history omits. Keep
// them only in bounded memory; none are returned in Evidence or error text.
type childStream struct {
	events []frame
	bytes  int
	err    error
}

const childStreamEventLimit = 512

type childProof uint8

const (
	childNotProven childProof = iota
	childPending
	childProven
)

func (stream *childStream) observe(message frame) error {
	if stream.err != nil {
		return stream.err
	}
	switch message.Method {
	case "rawResponseItem/completed", "turn/started", "turn/completed", "thread/closed":
		stream.bytes += message.size
		if len(stream.events) >= childStreamEventLimit || stream.bytes > 1<<20 {
			stream.err = errors.New("live: child stream budget exceeded")
			return stream.err
		}
		stream.events = append(stream.events, message)
	}
	return nil
}

func (stream *childStream) proof(child, parent productThread, credited productTurn, reply, nonce, setupMarker string) childProof {
	type observedTurn struct {
		terminalError                                      any
		started, ended, invalid                            bool
		status                                             string
		inputs                                             int
		reply                                              productTurn
		finalResultAt, legacyResultAt, endedAt, firstInput int
		finalMarkerAt, legacyMarkerAt                      int
		hasFinal                                           bool
	}
	turns := make(map[string]*observedTurn)
	var order []string
	var inputs []struct {
		text string
		at   int
	}
	invalidAt := len(stream.events)
	closedAt := len(stream.events)
	toolAt := len(stream.events)
	path := child.Source.SubAgent.ThreadSpawn.AgentPath
	for index, message := range stream.events {
		var event struct {
			ThreadID, TurnID string
			Turn             productTurn
			Item             struct {
				ID, Type, Role, Phase, Author, Recipient string
				Content                                  []struct{ Type, Text string }
			}
		}
		if json.Unmarshal(message.Params, &event) != nil {
			return childNotProven
		}
		if event.ThreadID != child.ID {
			continue
		}
		if message.Method == "thread/closed" {
			closedAt = min(closedAt, index)
			continue
		}
		id := event.TurnID
		if message.Method != "rawResponseItem/completed" {
			id = event.Turn.ID
		}
		if id == "" {
			return childNotProven
		}
		observed := turns[id]
		if observed == nil {
			observed = &observedTurn{finalResultAt: -1, legacyResultAt: -1, finalMarkerAt: -1, legacyMarkerAt: -1, endedAt: -1, firstInput: -1}
			turns[id] = observed
		}
		switch message.Method {
		case "turn/started":
			if observed.started || observed.ended || event.Turn.Status != "inProgress" {
				observed.invalid = true
			}
			observed.started = true
			order = append(order, id)
		case "turn/completed":
			var terminalError any
			if len(event.Turn.Error) != 0 && json.Unmarshal(event.Turn.Error, &terminalError) != nil {
				return childNotProven
			}
			if !observed.started || (observed.ended && (observed.status != event.Turn.Status || !reflect.DeepEqual(observed.terminalError, terminalError))) {
				observed.invalid = true
			}
			observed.ended, observed.status = true, event.Turn.Status
			observed.terminalError = terminalError
			if observed.endedAt < 0 {
				observed.endedAt = index
			}
		case "rawResponseItem/completed":
			if !observed.started || observed.ended {
				observed.invalid = true
			}
			item := event.Item
			switch item.Type {
			case "agent_message":
				if item.Author == "" || item.Recipient != path || len(item.Content) == 0 {
					invalidAt = min(invalidAt, index)
				}
				var text []string
				for _, part := range item.Content {
					if part.Type != "input_text" {
						invalidAt = min(invalidAt, index)
					}
					text = append(text, part.Text)
				}
				joined := strings.Join(text, "\n")
				if strings.TrimSpace(joined) == "" {
					invalidAt = min(invalidAt, index)
				}
				inputs = append(inputs, struct {
					text string
					at   int
				}{joined, index})
				observed.inputs++
				if observed.firstInput < 0 {
					observed.firstInput = index
				}
			case "message":
				if item.Role != "assistant" {
					// Host context is also input. It cannot newly supply either
					// proof value before the child produces that value itself.
					var text []string
					for _, part := range item.Content {
						if part.Type != "input_text" {
							invalidAt = min(invalidAt, index)
						}
						text = append(text, part.Text)
					}
					inputs = append(inputs, struct {
						text string
						at   int
					}{strings.Join(text, "\n"), index})
				}
				if item.Role != "assistant" || (item.Phase != "" && item.Phase != "final_answer") {
					continue
				}
				var text []string
				for _, part := range item.Content {
					if part.Type != "output_text" {
						observed.invalid = true
					}
					text = append(text, part.Text)
				}
				joined := strings.Join(text, "\n")
				public, _ := json.Marshal(map[string]string{"id": item.ID, "type": "agentMessage", "phase": item.Phase, "text": joined})
				observed.reply.Items = append(observed.reply.Items, public)
				if item.Phase == "final_answer" {
					observed.hasFinal = true
					if observed.finalResultAt < 0 && containsProofValue(joined, nonce) {
						observed.finalResultAt = index
					}
					if observed.finalMarkerAt < 0 && containsProofValue(joined, setupMarker) {
						observed.finalMarkerAt = index
					}
				} else {
					if observed.legacyResultAt < 0 && containsProofValue(joined, nonce) {
						observed.legacyResultAt = index
					}
					if observed.legacyMarkerAt < 0 && containsProofValue(joined, setupMarker) {
						observed.legacyMarkerAt = index
					}
				}
			case "reasoning":
				// Model reasoning is not another source of inherited context.
			default:
				// A pre-result tool/lookup path cannot prove inherited recall.
				toolAt = min(toolAt, index)
			}
		}
	}
	selected := turns[credited.ID]
	if selected == nil {
		if closedAt < len(stream.events) || invalidAt < len(stream.events) {
			return childNotProven
		}
		return childPending
	}
	if selected.invalid || (closedAt < len(stream.events) && (!selected.ended || closedAt < selected.endedAt)) {
		return childNotProven
	}
	nonceAt, markerAt := selected.legacyResultAt, selected.legacyMarkerAt
	if selected.hasFinal {
		nonceAt, markerAt = selected.finalResultAt, selected.finalMarkerAt
	}
	resultAt := max(nonceAt, markerAt)
	if invalidAt < resultAt || (resultAt < 0 && invalidAt < len(stream.events)) {
		return childNotProven
	}
	for _, input := range inputs {
		if ((nonceAt < 0 || input.at < nonceAt) && containsProofValue(input.text, nonce)) ||
			((markerAt < 0 || input.at < markerAt) && containsProofValue(input.text, setupMarker)) {
			return childNotProven
		}
	}
	if (resultAt < 0 && toolAt < len(stream.events)) || toolAt < resultAt {
		return childNotProven
	}
	if !selected.ended {
		return childPending
	}
	if selected.status != "completed" || selected.terminalError != nil || nonceAt < 0 || markerAt < 0 || selected.firstInput < 0 || selected.firstInput >= min(nonceAt, markerAt) {
		return childNotProven
	}
	rawReply, err := finalProductReply(selected.reply)
	if err != nil || rawReply != reply {
		return childNotProven
	}
	// Only this child's order is relevant. Every own durable turn through the
	// credited turn needs its observed lifecycle and consumed input; a missed
	// initial attachment or partial stream cannot infer freshness from history.
	own := 0
	for _, turn := range child.Turns {
		inherited := false
		for _, prior := range parent.Turns {
			inherited = inherited || prior.ID == turn.ID
		}
		if inherited {
			// The stock fork retains user input and final assistant messages.
			// Public history also includes diagnostics that are not model input.
			for _, raw := range turn.Items {
				var item struct {
					Type, Phase, Text string
					Content           []struct{ Type, Text string }
				}
				if json.Unmarshal(raw, &item) != nil {
					return childNotProven
				}
				if item.Type == "agentMessage" && item.Phase == "final_answer" && containsProofValue(item.Text, nonce) {
					return childNotProven
				}
				if item.Type == "userMessage" {
					for _, part := range item.Content {
						if part.Type != "text" || containsProofValue(part.Text, nonce) {
							return childNotProven
						}
					}
				}
			}
			continue
		}
		observed := turns[turn.ID]
		var durableError any
		if len(turn.Error) != 0 && json.Unmarshal(turn.Error, &durableError) != nil {
			return childNotProven
		}
		if own >= len(order) || order[own] != turn.ID || observed == nil || !observed.started || !observed.ended || observed.invalid || observed.inputs == 0 || observed.status != turn.Status || !reflect.DeepEqual(observed.terminalError, durableError) {
			return childNotProven
		}
		own++
		if turn.ID == credited.ID {
			break
		}
	}
	return childProven
}
