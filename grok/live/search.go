package live

import (
	"context"
	"errors"

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

// HostedSearch submits one supported Web search and one follow-up, each once.
// Product Hosted X is retired; direct backend X probes belong to Facts.
// Its isolated fixture has no local tool executor. Native config/wire/history
// composition remains a prerequisite; this observer does not infer it from text.
func HostedSearch(ctx context.Context, options Options, scenario string) (SearchEvidence, error) {
	if scenario == "x" || scenario == "x_window" {
		return SearchEvidence{Scenario: scenario}, failedObservation("hosted_x_retired", "live: product hosted X is retired")
	}
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
	calls                                  map[string]map[string]bool
	frames, bytes                          int
	terminal, settled                      bool
}

func (p *searchProbe) observe(prompt string) error {
	p.evidence.Turns++
	response, err := p.server.client.Turns().Start(p.server.ctx, protocolv2.TurnStartParams{ThreadID: p.threadID,
		Input: []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{Text: prompt})}})
	if err != nil {
		return shippedTransport(p.server.ctx, err)
	}
	return p.observeStarted(response.Turn)
}

func (p *searchProbe) observeStarted(value protocolv2.Turn) error {
	p.frames, p.bytes, p.terminal, p.settled = 0, 0, false, false
	started, err := searchTurnFromSDK(value)
	if err != nil {
		return errors.New("live: invalid search evidence")
	}
	if started.ID == "" || started.ID == p.previousID {
		return errors.New("live: search turn identity unavailable")
	}
	p.turnID, p.evidence.Stage = started.ID, "search_turn_submitted"
	if p.calls == nil {
		p.calls = map[string]map[string]bool{}
	}
	// Keep the previous settled turn's credited identities through the single
	// continuation. A reused item ID in a new turn is a separate occurrence.
	p.calls[p.turnID] = map[string]bool{}

	// A start response is only a snapshot. Admit completion only through a
	// matching turn/completed notification, including for a terminal-looking start.
	for !p.terminal {
		message, err := p.server.next()
		if err != nil {
			return err
		}
		if err := p.notification(message); err != nil {
			return err
		}
	}
	// A snapshot can contradict the observed completion, but cannot replace it.
	if started.Status != "inProgress" {
		if err := validateSearchTerminal(started); err != nil {
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
	if read.Thread.ID != p.threadID || len(turns) > 32 {
		return errors.New("live: settled search history unavailable")
	}
	currentIndex, previousFound := -1, false
	for index, value := range turns {
		if value.ID == p.previousID && p.previousID != "" {
			if previousFound {
				return errors.New("live: search history identity duplicated")
			}
			previousFound = true
			if err := validateSearchTerminal(value); err != nil {
				return err
			}
		}
		if value.ID == p.turnID {
			if currentIndex >= 0 {
				return errors.New("live: search turn identity duplicated")
			}
			currentIndex = index
			if err := validateSearchTerminal(value); err != nil {
				return err
			}
			for _, item := range read.Thread.Turns[index].Items {
				if err := p.publicSearch(value.ID, item); err != nil {
					return err
				}
			}
		}
	}
	if currentIndex < 0 || p.previousID != "" && !previousFound {
		return errors.New("live: search history identity unavailable")
	}
	current, err := productTurnFromSDK(read.Thread.Turns[currentIndex])
	if err != nil {
		return errors.New("live: invalid search evidence")
	}
	reply, err := finalProductReply(current)
	if err != nil {
		return err
	}
	p.evidence.ReplyBytes = len(reply)
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
	if value, ok := message.Notification.AsTurnCompleted(); ok {
		if value.Params.ThreadID != p.threadID {
			return nil
		}
		terminal, err := searchTurnFromSDK(value.Params.Turn)
		if err != nil {
			return errors.New("live: invalid search evidence")
		}
		if terminal.ID == p.turnID {
			if err := p.complete(terminal); err != nil {
				return err
			}
			for _, item := range value.Params.Turn.Items {
				if err := p.publicSearch(terminal.ID, item); err != nil {
					return err
				}
			}
		} else if terminal.ID == p.previousID {
			return validateSearchTerminal(terminal)
		}
	} else if value, ok := message.Notification.AsItemCompleted(); ok {
		if value.Params.ThreadID == p.threadID && value.Params.TurnID == p.turnID {
			return p.publicSearch(value.Params.TurnID, value.Params.Item)
		}
	}
	return nil
}

// A completed public WebSearch item is the supported product result. Raw
// extension envelopes are not part of this business oracle.
func (p *searchProbe) publicSearch(turnID string, item protocolv2.ThreadItem) error {
	search, ok := item.AsWebSearch()
	if !ok {
		return nil
	}
	if search.ID == "" {
		return errors.New("live: search item identity unavailable")
	}
	credited := p.calls[turnID]
	if _, exists := credited[search.ID]; exists {
		return nil
	}
	// Settled history has already decided this turn's result. Late callbacks
	// do not add credit or turn an unrelated activity into a semantic failure.
	if turnID != p.turnID || p.settled {
		return nil
	}
	if len(credited) >= 64 {
		return errors.New("live: hosted search item budget exceeded")
	}
	credited[search.ID] = true
	p.result.CanonicalCalls++
	return nil
}

// Settled and terminal-summary evidence is authoritative even without a matching
// item notification. Validate it without changing the current probe identity.
func validateSearchTerminal(value turn) error {
	if value.Status != "completed" || len(value.Error) != 0 && string(value.Error) != "null" {
		return failedTurn(value.Status, value.Error, "live: search turn did not complete")
	}
	return nil
}
