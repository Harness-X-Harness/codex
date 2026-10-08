package live

import (
	"context"
	"encoding/json"
	"errors"
	"strings"

	"github.com/Harness-X-Harness/codex/grok/internal/searchfixture"
)

// SearchEvidence observes canonical hosted output and same-thread continuation.
// It does not independently inspect HTTP replay or establish a packaged Story.
type SearchEvidence struct {
	Evidence
	Scenario                     string
	CanonicalCalls, SettledTurns int
	FollowupCompleted            bool
}

// HostedSearch submits one fixed Web/X search and one follow-up, each once.
// Its isolated fixture has no local tool executor. Native config/wire/history
// composition remains a prerequisite; this observer does not infer it from text.
func HostedSearch(ctx context.Context, options Options, scenario string) (SearchEvidence, error) {
	result := SearchEvidence{}
	_, config, prompt, err := searchfixture.Plan(scenario)
	if err != nil {
		return result, errors.New("live: invalid hosted search fixture")
	}
	result.Scenario = scenario
	base, err := runFixture(ctx, options, nil, map[string]any{"experimentalRawEvents": true, "config": config}, func(server *appServer, threadID string, evidence *Evidence) error {
		probe := searchProbe{server: server, threadID: threadID, scenario: scenario, evidence: evidence, result: &result}
		if err := probe.observe(prompt); err != nil {
			return err
		}
		evidence.FirstCompleted, evidence.FirstReplyBytes = true, evidence.ReplyBytes
		if result.CanonicalCalls == 0 {
			return errors.New("live: completed hosted search absent")
		}
		evidence.Stage = "hosted_search_observed"
		probe.previousID = probe.turnID
		if err := probe.observe("Summarize the previous search result from this thread without searching again."); err != nil {
			return err
		}
		result.FollowupCompleted, evidence.Completed = true, true
		evidence.Stage = "hosted_continuation_completed"
		return nil
	})
	result.Evidence = base
	return result, err
}

type searchProbe struct {
	server                                 *appServer
	threadID, turnID, previousID, scenario string
	evidence                               *Evidence
	result                                 *SearchEvidence
	calls                                  map[string]string
	frames, bytes                          int
	terminal                               bool
}

func (p *searchProbe) observe(prompt string) error {
	p.evidence.Turns++
	p.calls, p.frames, p.bytes, p.terminal = map[string]string{}, 0, 0, false
	var started struct {
		Turn turn `json:"turn"`
	}
	if err := p.server.call("turn/start", map[string]any{"threadId": p.threadID, "input": []any{map[string]any{"type": "text", "text": prompt, "textElements": []any{}}}}, &started); err != nil {
		return err
	}
	if started.Turn.ID == "" || started.Turn.ID == p.previousID {
		return errors.New("live: search turn identity unavailable")
	}
	p.turnID, p.evidence.Stage = started.Turn.ID, "search_turn_submitted"
	if started.Turn.Status != "inProgress" {
		if err := p.complete(started.Turn); err != nil {
			return err
		}
	}
	for !p.terminal {
		message, err := p.server.next()
		if err != nil {
			return err
		}
		if err := p.notification(message); err != nil {
			return err
		}
	}
	// One settled read fences queued events and provides authoritative final text.
	// It is not a second semantic invocation or a scan of unrelated session files.
	var read struct {
		Thread struct {
			ID    string `json:"id"`
			Turns []turn `json:"turns"`
		} `json:"thread"`
	}
	if err := p.server.call("thread/read", map[string]any{"threadId": p.threadID, "includeTurns": true}, &read); err != nil {
		return err
	}
	for len(p.server.pending) > 0 {
		message, err := p.server.next()
		if err != nil {
			return err
		}
		if err := p.notification(message); err != nil {
			return err
		}
	}
	if read.Thread.ID != p.threadID || len(read.Thread.Turns) != p.evidence.Turns {
		return errors.New("live: settled search history unavailable")
	}
	if p.previousID != "" {
		previous := read.Thread.Turns[0]
		if previous.ID != p.previousID {
			return errors.New("live: search history identity changed")
		}
		if err := validateSearchTerminal(previous); err != nil {
			return err
		}
	}
	current := read.Thread.Turns[len(read.Thread.Turns)-1]
	if err := p.complete(current); err != nil {
		return err
	}
	p.evidence.ReplyBytes = 0
	for _, item := range current.Items {
		if item.Type == "agentMessage" {
			p.evidence.ReplyBytes = 0
			if item.Phase == "" || item.Phase == "final_answer" {
				p.evidence.ReplyBytes = len(strings.TrimSpace(item.Text))
			}
		}
	}
	if p.evidence.ReplyBytes == 0 {
		return errors.New("live: final search reply absent")
	}
	p.result.SettledTurns++
	return nil
}

func (p *searchProbe) complete(value turn) error {
	if value.ID != p.turnID {
		return errors.New("live: search turn identity changed")
	}
	if err := validateSearchTerminal(value); err != nil {
		return err
	}
	p.terminal = true
	p.evidence.Stage = "search_turn_completed"
	return nil
}

func (p *searchProbe) notification(message frame) error {
	p.frames++
	p.bytes += message.size
	if p.frames > 4096 || p.bytes > 64<<20 {
		return errors.New("live: search evidence budget exceeded")
	}
	if len(message.ID) != 0 {
		return p.server.refuse(message.ID)
	}
	if message.Method != "rawResponseItem/completed" && message.Method != "turn/completed" && message.Method != "item/completed" && message.Method != "item/started" {
		return nil
	}
	var event struct {
		ThreadID string          `json:"threadId"`
		TurnID   string          `json:"turnId"`
		Turn     turn            `json:"turn"`
		Item     json.RawMessage `json:"item"`
	}
	if json.Unmarshal(message.Params, &event) != nil {
		return errors.New("live: invalid search evidence")
	}
	if event.ThreadID != p.threadID {
		return nil
	}
	if message.Method == "turn/completed" {
		if event.Turn.ID == p.turnID {
			return p.complete(event.Turn)
		}
		if p.previousID != "" && event.Turn.ID == p.previousID {
			return validateSearchTerminal(event.Turn)
		}
		return nil
	}
	if event.TurnID != p.turnID && (p.previousID == "" || event.TurnID != p.previousID) {
		return nil
	}
	var kind struct {
		Type string `json:"type"`
	}
	if json.Unmarshal(event.Item, &kind) != nil {
		return errors.New("live: invalid search item")
	}
	if localSearchItem(kind.Type) {
		return errors.New("live: local execution cannot establish hosted search")
	}
	if event.TurnID != p.turnID || message.Method != "rawResponseItem/completed" {
		return nil
	}
	call, ok := searchfixture.ReplayCall(event.Item, p.scenario)
	if !ok {
		return nil
	}
	encoded, _ := json.Marshal(call)
	id := call["id"].(string)
	if old, exists := p.calls[id]; exists {
		if old != string(encoded) {
			return errors.New("live: conflicting hosted search evidence")
		}
		return nil
	}
	if len(p.calls) == 64 {
		return errors.New("live: hosted search item budget exceeded")
	}
	p.calls[id] = string(encoded)
	p.result.CanonicalCalls++
	return nil
}

// Settled and terminal-summary evidence is authoritative even without a matching
// item notification. Validate it without changing the current probe identity.
func validateSearchTerminal(value turn) error {
	if value.Status != "completed" || len(value.Error) != 0 && string(value.Error) != "null" {
		return errors.New("live: search turn did not complete")
	}
	for _, item := range value.Items {
		if localSearchItem(item.Type) {
			return errors.New("live: local execution cannot establish hosted search")
		}
	}
	return nil
}

func localSearchItem(kind string) bool {
	switch kind {
	case "function_call", "function_call_output", "custom_tool_call_output":
		return true
	// Public ThreadItem variants from app-server-protocol/src/protocol/v2/item.rs.
	// All these local execution capabilities are disabled in this isolated fixture.
	case "functionCallOutput", "dynamicToolCall", "mcpToolCall", "commandExecution",
		"fileChange", "imageView", "imageGeneration", "collabAgentToolCall", "subAgentActivity", "sleep":
		return true
	default:
		return false
	}
}
