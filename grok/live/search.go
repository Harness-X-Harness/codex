package live

import (
	"context"
	"encoding/json"
	"errors"
	"net/url"
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
	schema, err := searchOutputSchema(true)
	if err != nil {
		return err
	}
	p.server.diagnostics.Record("search_followup_request", map[string]any{"thread_id": p.threadID, "prompt": prompt, "output_schema": schema})
	p.evidence.Turns++
	response, err := p.server.client.Turns().Start(p.server.ctx, protocolv2.TurnStartParams{ThreadID: p.threadID, OutputSchema: &schema,
		Input: []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{Text: prompt})}})
	if err != nil {
		return shippedTransport(p.server.ctx, err)
	}
	return p.observeStarted(response.Turn)
}

func (p *searchProbe) observeStarted(value protocolv2.Turn) error {
	if p.server != nil {
		recordSDKDiagnosticTurn(p.server.diagnostics, "search_start_snapshot", p.threadID, value)
	}
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
	p.server.diagnostics.Record("search_read_snapshot", map[string]any{"thread_id": read.Thread.ID, "expected_thread_id": p.threadID, "turn_count": len(read.Thread.Turns), "omitted_turns": max(0, len(read.Thread.Turns)-32)})
	for _, value := range read.Thread.Turns[:min(32, len(read.Thread.Turns))] {
		recordSDKDiagnosticTurn(p.server.diagnostics, "search_read_turn", read.Thread.ID, value)
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
	if read.Thread.ID != p.threadID {
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
	recordProductDiagnosticTurn(p.server.diagnostics, "search_settled", p.threadID, current)
	reply, err := finalProductReply(current)
	if err != nil {
		return err
	}
	p.server.diagnostics.Record("search_answer", map[string]any{"thread_id": p.threadID, "turn_id": p.turnID, "text": reply})
	answerErr := validateSearchAnswer(reply, p.previousID != "")
	validation := map[string]any{"thread_id": p.threadID, "turn_id": p.turnID, "followup": p.previousID != "", "accepted": answerErr == nil, "json_valid": json.Valid([]byte(reply)), "parsed": DescribeFailure(answerErr).Code != "structured_reply_invalid"}
	if answerErr != nil {
		validation["failure"] = DescribeFailure(answerErr)
	}
	p.server.diagnostics.Record("search_answer_validation", validation)
	if answerErr != nil {
		return answerErr
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
		return failedObservation("observation_budget_exceeded", "live: search evidence budget exceeded")
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
	raw, _ := json.Marshal(item)
	if p.server != nil {
		p.server.diagnostics.Item("search_public_item", raw)
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

const searchArticlesSchema = `{"type":"object","properties":{"articles":{"type":"array","items":{"type":"object","properties":{"headline":{"type":"string"},"url":{"type":"string"}},"required":["headline","url"],"additionalProperties":false}},"summary":{"type":["string","null"]}},"required":["articles","summary"],"additionalProperties":false}`
const searchSummarySchema = `{"type":"object","properties":{"summary":{"type":["string","null"]}},"required":["summary"],"additionalProperties":false}`

func searchOutputSchema(followup bool) (protocolv2.OutputSchema, error) {
	schema := searchArticlesSchema
	if followup {
		schema = searchSummarySchema
	}
	return protocolv2.OutputSchemaFromJSON([]byte(schema))
}

func validateSearchAnswer(reply string, followup bool) error {
	invalid := func() error {
		return failedObservation("search_answer_invalid", "live: search answer does not satisfy its structured result contract")
	}
	if followup {
		var answer struct {
			Summary *string `json:"summary"`
		}
		if err := parseStructuredReply(reply, &answer, "summary"); err != nil {
			return err
		}
		if answer.Summary == nil || strings.TrimSpace(*answer.Summary) == "" {
			return invalid()
		}
		return nil
	}
	var answer struct {
		Articles []json.RawMessage `json:"articles"`
		Summary  *string           `json:"summary"`
	}
	if err := parseStructuredReply(reply, &answer, "articles", "summary"); err != nil {
		return err
	}
	if len(answer.Articles) == 0 {
		return invalid()
	}
	for _, raw := range answer.Articles {
		var article struct {
			Headline *string `json:"headline"`
			URL      *string `json:"url"`
		}
		if err := parseStructuredReply(string(raw), &article, "headline", "url"); err != nil {
			return err
		}
		if article.Headline == nil || strings.TrimSpace(*article.Headline) == "" || article.URL == nil {
			return invalid()
		}
		u, err := url.Parse(*article.URL)
		if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Hostname() == "" || u.User != nil {
			return invalid()
		}
	}
	return nil
}
