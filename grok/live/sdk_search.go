package live

import (
	"context"
	"encoding/json"
	"errors"
	"maps"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// sdkHostedSearch implements supported Web observation.
func sdkHostedSearch(ctx context.Context, options Options, scenario string, config map[string]any, prompt string) (SearchEvidence, error) {
	result := SearchEvidence{Scenario: scenario}
	options.Diagnostics.Record("search_request", map[string]any{"scenario": scenario, "config": config, "prompt": prompt, "model": options.Model})
	transport := &sdkProductObserver{diagnostics: options.Diagnostics, ctx: ctx, changed: make(chan struct{}, 1)}
	handlers := sdkFixtureHandlers{
		requests: transport.request, notifications: transport.typedNotification, afterClose: transport.afterClose,
		retention: codexsdk.NotificationRetentionBudget{MaxCount: 8192, MaxBytes: 256 << 20},
		// The business counter rejects the 4097th frame. The SDK default queue
		// of 64 closes the client before that frame is delivered.
		queue: 8192,
	}
	base, err := withSDKFixture(ctx, options, nil, handlers, func(client *codexsdk.Client, cwd string, evidence *Evidence) error {
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
			Turn: protocolv2.TurnStartParams{Input: []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{Text: prompt})}},
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
		probe := searchProbe{server: transport, threadID: started.Start.Thread.ID, scenario: scenario, evidence: evidence, result: &result}
		transport.finalize = func() error {
			latest, _ := stream.Result()
			return probe.revalidate(latest.Run.Notifications)
		}

		if err := probe.observeStarted(started.Run.Turn); err != nil {
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
	if err != nil {
		result.FollowupCompleted = false
	}
	result.Evidence = base
	return result, err
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
