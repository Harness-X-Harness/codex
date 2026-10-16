package live

import (
	"context"
	"encoding/json"
	"errors"
	"maps"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// sdkHostedSearch keeps the hosted Web/X oracle and replaces only its transport.
func sdkHostedSearch(ctx context.Context, options Options, scenario string, config map[string]any, prompt string) (SearchEvidence, error) {
	result := SearchEvidence{Scenario: scenario}
	transport := &sdkProductObserver{ctx: ctx, changed: make(chan struct{}, 1)}
	handlers := sdkFixtureHandlers{
		requests: transport.request, notifications: transport.typedNotification, afterClose: transport.afterClose,
		retention: codexsdk.NotificationRetentionBudget{MaxCount: 8192, MaxBytes: 256 << 20},
		// The business counter rejects the 4097th frame. The SDK default queue
		// of 64 closes the client before that frame is delivered.
		queue: 8192,
		raw: &codexsdk.RawNotificationSubscription{
			Methods:                        []string{codexsdk.RawResponseItemCompletedExtension},
			EnableRawResponseItemExtension: true, Handler: transport.rawNotification,
		},
	}
	base, err := withSDKFixture(ctx, options, nil, handlers, func(client *codexsdk.Client, cwd string, evidence *Evidence) error {
		transport.client = client
		encoded, err := json.Marshal(config)
		var decoded map[string]protocolv2.JSONValue
		if err != nil || json.Unmarshal(encoded, &decoded) != nil {
			return errors.New("live: invalid hosted search fixture")
		}
		experimental := true
		evidence.Threads = 1
		stream, err := client.ThreadRunner().StartStream(ctx, codexsdk.StartThreadRunRequest{
			Thread: protocolv2.ThreadStartParams{
				Model: protocolv2.Value(options.Model), ModelProvider: protocolv2.Value("grok"), CWD: protocolv2.Value(cwd),
				ExperimentalRawEvents: &experimental, ApprovalPolicy: protocolv2.Value(protocolv2.NewAskForApprovalNever()),
				Config: protocolv2.Value(decoded),
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
			if _, err := stream.Wait(waitCtx); err != nil {
				runFailures <- err
			}
		}()
		defer func() { cancelWait(); <-waitDone }()
		probe := searchProbe{server: transport, threadID: started.Start.Thread.ID, scenario: scenario, evidence: evidence, result: &result}
		transport.finalize = func() error {
			latest, _ := stream.Result()
			return probe.revalidate(latest.Run.Notifications)
		}

		// A notification terminal must still pass the observer's thread/turn
		// correlation; the SDK snapshot alone cannot supply that business proof.
		firstTurn := started.Run.Turn
		for _, notification := range started.Run.Notifications {
			if _, terminal := notification.AsTurnCompleted(); terminal {
				firstTurn.Status, firstTurn.Items = protocolv2.TurnStatusInProgress, nil
				break
			}
		}
		if err := probe.observeStarted(firstTurn); err != nil {
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
	result.ExpectedUnsupported = ctx.Err() == nil && acceptedXSearchRejection(result, err)
	return result, err
}

// revalidate checks observed queue and SDK-retained typed contradictions without
// manufacturing proof. The SDK has no retained raw-notification accessor after
// fatal decode, so expected rejection cannot attest complete raw delivery.
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
		var message frame
		if err != nil || json.Unmarshal(raw, &message) != nil {
			return errors.New("live: invalid search evidence")
		}
		message.size = len(raw)
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
