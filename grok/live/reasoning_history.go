package live

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"strings"
)

const historyTool = "grok_history_probe"

// This bounds replay evidence, not product tool calls or retries.
const historyAssistantLimit = 4096

// ReasoningHistory observes two same-thread turns without semantic reinvocation.
// Native encrypted replay/tool eligibility and Subject provenance are prerequisites;
// deterministic fixtures do not establish real-provider Story success.
func ReasoningHistory(ctx context.Context, options Options) (Evidence, error) {
	threadOptions := map[string]any{
		"experimentalRawEvents": true,
		"dynamicTools": []any{map[string]any{
			"type": "function", "name": historyTool,
			"description": "Return a fresh token for this history probe.",
			"inputSchema": map[string]any{"type": "object", "properties": map[string]any{}, "additionalProperties": false},
		}},
	}
	return runFixture(ctx, options, threadOptions, func(server *appServer, threadID string, evidence *Evidence) error {
		probe := historyProbe{server: server, threadID: threadID, evidence: evidence}
		if err := probe.observe("Call grok_history_probe, reason about its returned token, then include the exact token in your final reply."); err != nil {
			return err
		}
		evidence.Stage = "first_turn_proven"
		probe.previousID, probe.turnID = probe.turnID, ""
		if err := probe.observe("Recall the exact token returned by the tool in the previous turn from this thread's history. Include it in your final reply without calling a tool."); err != nil {
			return err
		}
		evidence.Stage = "history_recalled"
		return nil
	})
}

// Only the token, current turn identities and bounded reply evidence survive frames.
// Repeated internal calls are counted, never accumulated in a call-ID/event map.
type historyProbe struct {
	server                      *appServer
	evidence                    *Evidence
	threadID, turnID, previousID string
	token                       string
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

func (probe *historyProbe) toolRequest(message frame) error {
	var call struct {
		ThreadID  string  `json:"threadId"`
		TurnID    string  `json:"turnId"`
		CallID    string  `json:"callId"`
		Namespace *string `json:"namespace"`
		Tool      string  `json:"tool"`
	}
	if message.Method != "item/tool/call" || json.Unmarshal(message.Params, &call) != nil ||
		call.ThreadID != probe.threadID || call.TurnID == "" || call.TurnID == probe.previousID ||
		(probe.turnID != "" && call.TurnID != probe.turnID) || call.CallID == "" ||
		call.Namespace != nil || call.Tool != historyTool {
		return probe.server.refuse(message.ID)
	}
	// Requests may precede turn/start's reply. Reconcile this provisional binding
	// with the reply before accepting any proof, rather than blocking the server.
	probe.turnID = call.TurnID
	text, success := "History probe is unavailable during continuation.", false
	if probe.previousID == "" {
		if probe.token == "" {
			var nonce [32]byte
			if _, err := rand.Read(nonce[:]); err != nil {
				return errors.New("live: fresh tool result unavailable")
			}
			probe.token = hex.EncodeToString(nonce[:])
		}
		text, success = probe.token, true
		probe.evidence.ToolCalls++
	} else {
		probe.evidence.DeniedToolCalls++
	}
	result, _ := json.Marshal(map[string]any{"contentItems": []any{map[string]any{"type": "inputText", "text": text}}, "success": success})
	return probe.server.send(frame{ID: message.ID, Result: result})
}

func (probe *historyProbe) observe(prompt string) error {
	probe.server.request = probe.toolRequest
	defer func() { probe.server.request = nil }()
	probe.evidence.Turns++
	var started struct {
		Turn historyTurn `json:"turn"`
	}
	if err := probe.server.call("turn/start", map[string]any{
		"threadId": probe.threadID, "effort": "medium",
		"input": []any{map[string]any{"type": "text", "text": prompt, "textElements": []any{}}},
	}, &started); err != nil {
		return err
	}
	if started.Turn.ID == "" || started.Turn.ID == probe.previousID || (probe.turnID != "" && probe.turnID != started.Turn.ID) {
		return errors.New("live: turn identity not established")
	}
	probe.turnID = started.Turn.ID
	probe.evidence.Stage = "turn_submitted"
	if probe.previousID != "" {
		probe.evidence.Stage = "continuation_submitted"
	}
	reply := historyReply{identities: make(map[[sha256.Size]byte]historyReplyOutcome)}
	// Include partial evidence on every exit, including an item-level failure.
	defer func() {
		if reply.completed {
			probe.evidence.Stage = "first_turn_completed"
			if probe.previousID != "" {
				probe.evidence.Stage = "continuation_completed"
			}
		}
		recall := reply.recall && !reply.ambiguous
		if probe.previousID == "" {
			probe.evidence.FirstCompleted, probe.evidence.FirstRecall, probe.evidence.FirstReplyBytes = reply.completed, recall, reply.bytes
		} else {
			probe.evidence.Completed, probe.evidence.Recalled, probe.evidence.ReplyBytes = reply.completed, recall, reply.bytes
		}
	}()
	if started.Turn.Status != "inProgress" {
		if err := probe.completedTurn(started.Turn, &reply); err != nil {
			return err
		}
	}
	for {
		if reply.authoritative && !reply.recall {
			return errors.New("live: final reply did not recall tool result")
		}
		firstProof := probe.evidence.ReasoningItems > 0 && probe.evidence.EncryptedItems > 0 && probe.evidence.CompletedTools > 0
		// Drain already queued notifications even if the inline reply completed.
		if len(probe.server.pending) == 0 && reply.completed && reply.recall && !reply.ambiguous && (probe.previousID != "" || firstProof) {
			return nil
		}
		message, err := probe.server.next()
		if err != nil {
			return err
		}
		if len(message.ID) != 0 {
			if err := probe.toolRequest(message); err != nil {
				return err
			}
			continue
		}
		if message.Method != "turn/completed" && message.Method != "item/completed" && message.Method != "rawResponseItem/completed" {
			continue
		}
		var completed struct {
			ThreadID string      `json:"threadId"`
			TurnID   string      `json:"turnId"`
			Turn     historyTurn `json:"turn"`
			Item     historyItem `json:"item"`
		}
		if json.Unmarshal(message.Params, &completed) != nil {
			return errors.New("live: invalid history evidence")
		}
		if completed.ThreadID != probe.threadID {
			continue
		}
		if message.Method == "turn/completed" {
			if completed.Turn.ID == probe.turnID {
				if err := probe.completedTurn(completed.Turn, &reply); err != nil {
					return err
				}
			}
		} else if completed.TurnID == probe.turnID {
			if message.Method == "rawResponseItem/completed" {
				if probe.previousID == "" && completed.Item.Type == "reasoning" {
					probe.evidence.ReasoningItems++
					if completed.Item.EncryptedContent != "" {
						probe.evidence.EncryptedItems++
					}
				}
			} else if err := probe.completedItem(completed.Item, &reply); err != nil {
				return err
			}
		}
	}
}

func (probe *historyProbe) completedTurn(turn historyTurn, reply *historyReply) error {
	if turn.Status != "completed" || len(turn.Error) != 0 && string(turn.Error) != "null" {
		return errors.New("live: turn did not complete successfully")
	}
	reply.completed = true
	// A terminal summary's last assistant message is authoritative, even if an
	// earlier item recalled the token or an older notification is replayed later.
	for _, item := range turn.Items {
		if item.Type == "agentMessage" {
			if item.ID == "" {
				return errors.New("live: assistant item identity not established")
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
			return errors.New("live: assistant item identity not established")
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
			return errors.New("live: assistant identity evidence budget exceeded")
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
