package live

import (
	"context"
	"encoding/json"
	"errors"
	"maps"
	"net/url"
	"strings"

	"github.com/Harness-X-Harness/codex/grok/internal/searchfixture"
	"github.com/ronhuafeng/llm-go/codexsdk"
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
	result := SearchEvidence{Scenario: scenario}
	schema, err := searchOutputSchema(false)
	if err != nil {
		return result, err
	}
	prompt += " Include each article's headline and link, and a short summary."
	options.Diagnostics.Record("search_request", map[string]any{"scenario": scenario, "config": config, "prompt": prompt, "model": options.Model, "output_schema": schema})
	transport := &sdkProductObserver{diagnostics: options.Diagnostics, ctx: ctx, changed: make(chan struct{}, 1)}
	handlers := sdkFixtureHandlers{
		requests: transport.request, notifications: transport.typedNotification,
		retention: codexsdk.NotificationRetentionBudget{MaxCount: 8192, MaxBytes: 256 << 20},
		// The business counter rejects the 4097th frame. The SDK default queue
		// of 64 closes the client before that frame is delivered.
		queue: 8192,
	}
	base, err := withSDKFixture(ctx, options, nil, handlers, func(client *codexsdk.Client, cwd string, evidence *Evidence, closeSDK func() error) (runErr error) {
		defer func() {
			if finalErr := closeSDK(); finalErr != nil {
				runErr = preferObservation(runErr, finalErr)
			}
			if evidence.CleanupPending {
				evidence.Completed = false
				return
			}
			if finalErr := transport.afterClose(cwd, evidence, runErr); finalErr != nil {
				runErr = preferObservation(runErr, finalErr)
			}
		}()
		transport.client = client
		encoded, err := json.Marshal(config)
		var decoded map[string]protocolv2.JSONValue
		if err != nil || json.Unmarshal(encoded, &decoded) != nil {
			return errors.New("live: invalid hosted search fixture")
		}
		evidence.Threads = 1
		stream, err := client.ThreadRunner().StartStream(ctx, codexsdk.StartThreadRunRequest{
			Thread: protocolv2.ThreadStartParams{
				Model: protocolv2.Value(options.Model), ModelProvider: protocolv2.Value("grok"), CWD: protocolv2.Value(cwd),
				ApprovalPolicy: protocolv2.Value(protocolv2.NewAskForApprovalNever()),
				Config:         protocolv2.Value(decoded),
			},
			Turn: protocolv2.TurnStartParams{OutputSchema: &schema, Input: []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{Text: prompt})}},
			AdmitTurn: func(started protocolv2.ThreadStartResponse, _ protocolv2.TurnStartParams) error {
				if started.Model != options.Model || started.ModelProvider != "grok" || started.Thread.ModelProvider != "grok" ||
					started.Thread.ID == "" || started.CWD != cwd || started.Thread.CWD != cwd {
					return errors.New("live: fixture binding not established")
				}
				evidence.Bound, evidence.Stage, evidence.Turns = true, "thread_bound", 1
				return nil
			},
		})
		if err != nil {
			return shippedTransport(ctx, err)
		}
		defer stream.Close()
		started, ok := stream.Result()
		if !ok || started.Run.Turn.ID == "" {
			return errors.New("live: search turn identity unavailable")
		}
		// Wait is the public early-failure signal even if decoding fails before
		// the first callback. It submits no additional RPC or semantic work.
		waitCtx, cancelWait := context.WithCancel(ctx)
		waitDone, runFailures := make(chan struct{}), make(chan error, 1)
		transport.runFailures = runFailures
		go func() {
			defer close(waitDone)
			_, waitErr := stream.Wait(waitCtx)
			if err := searchRunTransportFailure(waitErr, started.Start.Thread.ID, started.Run.Turn.ID); err != nil {
				runFailures <- err
			}
		}()
		defer func() { cancelWait(); <-waitDone }()
		probe := searchProbe{server: transport, threadID: started.Start.Thread.ID, evidence: evidence, result: &result}
		// Failure paths retain the same contradiction evidence as success paths.
		// This runs before deferred stream cancellation.
		defer func() {
			if runErr == nil {
				return
			}
			if finalErr := closeSDK(); finalErr != nil {
				runErr = preferObservation(runErr, finalErr)
			}
			if evidence.CleanupPending {
				return
			}
			observed := transport.afterClose(cwd, evidence, runErr)
			if observed == nil {
				latest, _ := stream.Result()
				observed = probe.revalidate(latest.Run.Notifications)
			}
			if finalErr := observed; finalErr != nil {
				runErr = preferObservation(runErr, finalErr)
			}
		}()

		articles, err := probe.awaitReply(started.Run.Turn)
		if err != nil {
			return err
		}
		answerErr := validateSearchAnswer(articles, false)
		recordSearchValidation(options.Diagnostics, probe.threadID, probe.turnID, articles, false, answerErr)
		if answerErr != nil {
			return answerErr
		}
		evidence.ReplyBytes, probe.settled = len(articles), true
		result.SettledTurns++
		evidence.FirstCompleted, evidence.FirstReplyBytes = true, evidence.ReplyBytes
		if result.CanonicalCalls == 0 {
			return errors.New("live: completed hosted search absent")
		}
		evidence.Stage = "hosted_search_observed"
		probe.previousID = probe.turnID
		summarySchema, err := searchOutputSchema(true)
		if err != nil {
			return err
		}
		followup := "Summarize the previous search result from this thread without searching again."
		options.Diagnostics.Record("search_followup_request", map[string]any{"thread_id": probe.threadID, "prompt": followup, "output_schema": summarySchema})
		evidence.Turns++
		continued, err := client.Turns().Start(ctx, protocolv2.TurnStartParams{
			ThreadID: probe.threadID, OutputSchema: &summarySchema,
			Input: []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{Text: followup})},
		})
		if err != nil {
			return shippedTransport(ctx, err)
		}
		summary, err := probe.awaitReply(continued.Turn)
		if err != nil {
			return err
		}
		answerErr = validateSearchAnswer(summary, true)
		recordSearchValidation(options.Diagnostics, probe.threadID, probe.turnID, summary, true, answerErr)
		if answerErr != nil {
			return answerErr
		}
		evidence.ReplyBytes, probe.settled = len(summary), true
		result.SettledTurns++

		// Join callbacks before the final contradiction-only read of retained evidence.
		if err := closeSDK(); err != nil {
			return err
		}
		if err := transport.afterClose(cwd, evidence, nil); err != nil {
			return err
		}
		latest, _ := stream.Result()
		if err := probe.revalidate(latest.Run.Notifications); err != nil {
			return err
		}
		result.FollowupCompleted, evidence.Completed = true, true
		evidence.Stage = "hosted_continuation_completed"
		return nil
	})
	if err != nil {
		result.FollowupCompleted = false
	}
	result.Evidence = base
	return result, err
}

type searchProbe struct {
	server                       *sdkProductObserver
	threadID, turnID, previousID string
	evidence                     *Evidence
	result                       *SearchEvidence
	calls                        map[string]map[string]bool
	frames, bytes                int
	terminal, settled            bool
}

func (p *searchProbe) awaitReply(value protocolv2.Turn) (string, error) {
	if p.server != nil {
		recordSDKDiagnosticTurn(p.server.diagnostics, "search_start_snapshot", p.threadID, value)
	}
	p.frames, p.bytes, p.terminal, p.settled = 0, 0, false, false
	started := value
	if started.ID == "" || started.ID == p.previousID {
		return "", errors.New("live: search turn identity unavailable")
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
			return "", err
		}
		if err := p.notification(message); err != nil {
			return "", err
		}
	}
	// A snapshot can contradict the observed completion, but cannot replace it.
	if started.Status != "inProgress" {
		if err := validateSearchTerminal(started); err != nil {
			return "", err
		}
	}
	// One settled read fences queued events and provides authoritative final text.
	// It is not a second semantic invocation or a scan of unrelated session files.
	include := true
	read, err := p.server.client.Threads().Read(p.server.ctx, protocolv2.ThreadReadParams{ThreadID: p.threadID, IncludeTurns: &include})
	if err != nil {
		return "", shippedTransport(p.server.ctx, err)
	}
	p.server.diagnostics.Record("search_read_snapshot", map[string]any{"thread_id": read.Thread.ID, "expected_thread_id": p.threadID, "turn_count": len(read.Thread.Turns), "omitted_turns": max(0, len(read.Thread.Turns)-32)})
	for _, value := range read.Thread.Turns[:min(32, len(read.Thread.Turns))] {
		recordSDKDiagnosticTurn(p.server.diagnostics, "search_read_turn", read.Thread.ID, value)
	}
	turns := read.Thread.Turns
	for len(p.server.queued()) > 0 {
		message, err := p.server.next()
		if err != nil {
			return "", err
		}
		if err := p.notification(message); err != nil {
			return "", err
		}
	}
	if read.Thread.ID != p.threadID {
		return "", errors.New("live: settled search history unavailable")
	}
	currentIndex, previousFound := -1, false
	for index, value := range turns {
		if value.ID == p.previousID && p.previousID != "" {
			if previousFound {
				return "", errors.New("live: search history identity duplicated")
			}
			previousFound = true
			if err := validateSearchTerminal(value); err != nil {
				return "", err
			}
		}
		if value.ID == p.turnID {
			if currentIndex >= 0 {
				return "", errors.New("live: search turn identity duplicated")
			}
			currentIndex = index
			if err := validateSearchTerminal(value); err != nil {
				return "", err
			}
			for _, item := range read.Thread.Turns[index].Items {
				if err := p.publicSearch(value.ID, item); err != nil {
					return "", err
				}
			}
		}
	}
	if currentIndex < 0 || p.previousID != "" && !previousFound {
		return "", errors.New("live: search history identity unavailable")
	}
	current, err := productTurnFromSDK(read.Thread.Turns[currentIndex])
	if err != nil {
		return "", errors.New("live: invalid search evidence")
	}
	recordProductDiagnosticTurn(p.server.diagnostics, "search_settled", p.threadID, current)
	reply, err := finalProductReply(current)
	if err != nil {
		return "", err
	}
	p.server.diagnostics.Record("search_answer", map[string]any{"thread_id": p.threadID, "turn_id": p.turnID, "text": reply})
	return reply, nil
}

func (p *searchProbe) complete(value protocolv2.Turn) error {
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
		terminal := value.Params.Turn
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
	if p.server != nil && p.server.diagnostics != nil {
		raw, _ := json.Marshal(item)
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
func validateSearchTerminal(value protocolv2.Turn) error {
	if value.Status != protocolv2.TurnStatusCompleted || value.Error != nil && value.Error.Value != nil {
		raw, _ := json.Marshal(value.Error)
		return failedTurn(string(value.Status), raw, "live: search turn did not complete")
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

// The SDK dispatches the matching terminal notification before Wait returns a
// TurnError. Leave that failure to the correlated observer; racing it through
// the transport channel would erase its terminal classification. Unrelated,
// wrapped, joined, and protocol/decode failures still take the early-failure path.
func searchRunTransportFailure(err error, threadID, turnID string) error {
	terminal, ok := err.(*codexsdk.TurnError)
	if ok && terminal != nil && terminal.ThreadID == threadID && terminal.Turn.ID == turnID &&
		(terminal.Turn.Status == protocolv2.TurnStatusFailed && terminal.Err == codexsdk.ErrTurnFailed ||
			terminal.Turn.Status == protocolv2.TurnStatusInterrupted && terminal.Err == codexsdk.ErrTurnInterrupted) {
		return nil
	}
	return err
}

// revalidate checks observed queue and SDK-retained typed contradictions without
// manufacturing proof. Raw compatibility is tested separately from this public
// product observation.
func (probe *searchProbe) revalidate(notifications []protocolv2.ServerNotification) error {
	check, savedResult, savedEvidence := *probe, *probe.result, *probe.evidence
	check.result, check.evidence = &savedResult, &savedEvidence
	check.calls = maps.Clone(probe.calls)
	for id, calls := range check.calls {
		check.calls[id] = maps.Clone(calls)
	}

	retained := check
	retained.frames, retained.bytes = 0, 0
	for _, notification := range notifications {
		raw, err := json.Marshal(notification)
		if err != nil {
			return errors.New("live: invalid search evidence")
		}
		message := frame{Method: string(notification.Kind()), Notification: notification, size: len(raw)}
		if err := retained.notification(message); err != nil {
			return err
		}
	}
	for _, message := range probe.server.queued() {
		if err := check.notification(message); err != nil {
			return err
		}
	}
	return nil
}

// Optional diagnostics describe, but never decide, the explicit business assertion.
func recordSearchValidation(diagnostics *Diagnostics, threadID, turnID, reply string, followup bool, answerErr error) {
	if diagnostics == nil {
		return
	}
	validation := map[string]any{"thread_id": threadID, "turn_id": turnID, "followup": followup, "accepted": answerErr == nil, "json_valid": json.Valid([]byte(reply)), "parsed": DescribeFailure(answerErr).Code != "structured_reply_invalid"}
	if answerErr != nil {
		validation["failure"] = DescribeFailure(answerErr)
	}
	diagnostics.Record("search_answer_validation", validation)
}
