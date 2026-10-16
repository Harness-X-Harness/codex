package live

import (
	"context"
	"encoding/json"
	"errors"
	"strings"

	"github.com/Harness-X-Harness/codex/grok/internal/searchfixture"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
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
	_, config, prompt, err := searchfixture.Plan(scenario)
	if err != nil {
		return SearchEvidence{}, errors.New("live: invalid hosted search fixture")
	}
	return sdkHostedSearch(ctx, options, scenario, config, prompt)
}

type searchProbe struct {
	server                                 *sdkProductObserver
	threadID, turnID, previousID, scenario string
	evidence                               *Evidence
	result                                 *SearchEvidence
	calls                                  map[string]map[string]string
	frames, bytes                          int
	terminal, settled                      bool
}

func (p *searchProbe) observe(prompt string) error {
	p.evidence.Turns++
	p.frames, p.bytes, p.terminal, p.settled = 0, 0, false, false
	response, err := p.server.client.Turns().Start(p.server.ctx, protocolv2.TurnStartParams{ThreadID: p.threadID,
		Input: []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{Text: prompt})}})
	if err != nil {
		return shippedTransport(p.server.ctx, err)
	}
	started, err := searchTurnFromSDK(response.Turn)
	if err != nil {
		return errors.New("live: invalid search evidence")
	}
	if started.ID == "" || started.ID == p.previousID {
		return errors.New("live: search turn identity unavailable")
	}
	p.turnID, p.evidence.Stage = started.ID, "search_turn_submitted"
	if p.calls == nil {
		p.calls = map[string]map[string]string{}
	}
	// Keep the previous settled turn's credited identities through the single
	// continuation. A reused item ID in a new turn is a separate occurrence.
	p.calls[p.turnID] = map[string]string{}
	if started.Status != "inProgress" {
		if err := p.complete(started); err != nil {
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
	include := true
	read, err := p.server.client.Threads().Read(p.server.ctx, protocolv2.ThreadReadParams{ThreadID: p.threadID, IncludeTurns: &include})
	if err != nil {
		return shippedTransport(p.server.ctx, err)
	}
	turns := make([]turn, 0, len(read.Thread.Turns))
	for _, value := range read.Thread.Turns {
		projected, err := searchTurnFromSDK(value)
		if err != nil {
			return errors.New("live: invalid search evidence")
		}
		turns = append(turns, projected)
	}
	for len(p.server.queued()) > 0 {
		message, err := p.server.next()
		if err != nil {
			return err
		}
		if err := p.notification(message); err != nil {
			return err
		}
	}
	if read.Thread.ID != p.threadID || len(turns) != p.evidence.Turns {
		return errors.New("live: settled search history unavailable")
	}
	if p.previousID != "" {
		previous := turns[0]
		if previous.ID != p.previousID {
			return errors.New("live: search history identity changed")
		}
		if err := validateSearchTerminal(previous); err != nil {
			return err
		}
	}
	current := turns[len(turns)-1]
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
	p.settled = true
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
		ID   string `json:"id"`
	}
	if json.Unmarshal(event.Item, &kind) != nil {
		return errors.New("live: invalid search item")
	}
	// Raw canonical hosted X is distinct from local custom execution, even
	// when the selected scenario credits Web. Public ThreadItem notifications
	// never use raw custom_tool_call; those remain rejected by localSearchItem.
	_, hostedX := searchfixture.ReplayCall(event.Item, "x")
	if localSearchItem(kind.Type) && !(message.Method == "rawResponseItem/completed" && hostedX) {
		return errors.New("live: local execution cannot establish hosted search")
	}
	if message.Method != "rawResponseItem/completed" {
		return nil
	}
	credited := p.calls[event.TurnID]
	old, exists := credited[kind.ID]
	call, ok := searchfixture.ReplayCall(event.Item, p.scenario)
	if !ok {
		if exists {
			return errors.New("live: conflicting hosted search evidence")
		}
		return nil
	}
	encoded, _ := json.Marshal(call)
	if exists {
		if old != string(encoded) {
			return errors.New("live: conflicting hosted search evidence")
		}
		return nil
	}
	if event.TurnID != p.turnID || p.settled {
		return errors.New("live: new hosted search evidence after settled turn")
	}
	if len(credited) == 64 {
		return errors.New("live: hosted search item budget exceeded")
	}
	credited[kind.ID] = string(encoded)
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
	case "function_call", "function_call_output", "custom_tool_call", "custom_tool_call_output":
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
