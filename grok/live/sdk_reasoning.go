package live

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/Harness-X-Harness/codex/grok/internal/integrationfixture"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func sdkReasoningHistory(ctx context.Context, options Options) (Evidence, error) {
	schema, schemaErr := protocolv2.OutputSchemaFromJSON([]byte(historyOutputSchema))
	if schemaErr != nil {
		return Evidence{}, failedObservation("history_schema_invalid", "live: history output schema unavailable")
	}
	observer := newSDKHistoryObserver()
	observer.probe.diagnostics = options.Diagnostics
	handlers := sdkFixtureHandlers{
		requests: observer.request, notifications: observer.typedNotification, failures: observer.failures,
		retention: codexsdk.NotificationRetentionBudget{MaxCount: 65536, MaxBytes: 64 << 20},
		raw: &codexsdk.RawNotificationSubscription{
			Methods:                        []string{codexsdk.RawResponseItemCompletedExtension},
			EnableRawResponseItemExtension: true, Handler: observer.notification,
		},
	}
	evidence, err := withSDKFixture(ctx, options, nil, handlers, func(client *codexsdk.Client, cwd string, evidence *Evidence) error {
		if options.shippedProfile {
			if err := requireSDKShippedCatalog(ctx, client, evidence); err != nil {
				return err
			}
		}
		observer.mu.Lock()
		observer.observations = *evidence
		observer.observations.Threads = 1
		observer.mu.Unlock()
		defer func() {
			observer.mu.Lock()
			defer observer.mu.Unlock()
			*evidence = observer.observations
		}()
		turn := protocolv2.TurnStartParams{
			OutputSchema: &schema,
			Input: []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{
				Text: "Check today's apple price at this shop. How much would two kilograms cost in yuan?",
			})},
		}
		options.Diagnostics.Record("turn_input", map[string]any{"scenario": "reasoning", "turn": 1, "model": options.Model, "input": turn.Input, "output_schema": json.RawMessage(historyOutputSchema)})
		options.Diagnostics.Record("history_tool_declaration", map[string]any{
			"tool": historyTool, "description": integrationfixture.HistoryProbeDescription,
			"declared_to_sdk": true, "provider_request_observed": false,
		})
		experimental := true
		stream, err := client.ThreadRunner().StartStream(ctx, codexsdk.StartThreadRunRequest{
			Thread: protocolv2.ThreadStartParams{
				Model: protocolv2.Value(options.Model), ModelProvider: protocolv2.Value("grok"), CWD: protocolv2.Value(cwd),
				ExperimentalRawEvents: &experimental,
				DynamicTools: protocolv2.Value([]protocolv2.DynamicToolSpec{protocolv2.NewDynamicToolSpecFunction(protocolv2.DynamicToolSpecFunction{
					Name: historyTool, Description: integrationfixture.HistoryProbeDescription,
					InputSchema: protocolv2.JSONObject(map[string]protocolv2.JSONValue{
						"type": protocolv2.JSONString("object"), "properties": protocolv2.JSONObject(map[string]protocolv2.JSONValue{}),
						"additionalProperties": protocolv2.JSONBool(false),
					}),
				})}),
			},
			Turn: turn,
			AdmitTurn: func(thread protocolv2.ThreadStartResponse, _ protocolv2.TurnStartParams) error {
				options.Diagnostics.Record("history_thread_config", map[string]any{"model": thread.Model, "provider": thread.ModelProvider, "reasoning_effort": thread.ReasoningEffort, "provider_request_observed": false})
				if thread.Thread.ID == "" || thread.Model != options.Model || thread.ModelProvider != "grok" || thread.Thread.ModelProvider != "grok" || thread.CWD != cwd {
					return failedObservation("fixture_binding_failed", "live: fixture binding not established")
				}
				observer.mu.Lock()
				defer observer.mu.Unlock()
				observer.probe.threadID = thread.Thread.ID
				observer.observations.Bound, observer.observations.Stage = true, "thread_bound"
				observer.observations.Turns++
				observer.active = true
				return nil
			},
		})
		if err != nil {
			return observer.failure(err)
		}
		if err := observeSDKHistoryTurn(ctx, observer, stream, func(run codexsdk.StartedThreadRun) codexsdk.ThreadRunResult { return run.Run }); err != nil {
			return err
		}
		threadID := observer.beginContinuation()
		turn.OutputSchema = &schema // Explicit on the resumed turn; never rely on inheritance.
		turn.Input = []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{
			Text: "Using the price we already checked, how much would three kilograms of apples cost? Do not look up the price again.",
		})}
		options.Diagnostics.Record("turn_input", map[string]any{"scenario": "reasoning", "turn": 2, "model": options.Model, "input": turn.Input, "output_schema": json.RawMessage(historyOutputSchema)})
		excludeTurns := true
		continued, err := client.ThreadRunner().ResumeStream(ctx, codexsdk.ResumeThreadRunRequest{
			Thread: protocolv2.ThreadResumeParams{ThreadID: threadID, Model: protocolv2.Value(options.Model), ModelProvider: protocolv2.Value("grok"), CWD: protocolv2.Value(cwd), ExcludeTurns: &excludeTurns},
			Turn:   turn,
			AdmitTurn: func(thread protocolv2.ThreadResumeResponse, _ protocolv2.TurnStartParams) error {
				if thread.Thread.ID != threadID || thread.Model != options.Model || thread.ModelProvider != "grok" || thread.Thread.ModelProvider != "grok" || thread.CWD != cwd {
					return failedObservation("fixture_binding_failed", "live: fixture binding not established")
				}
				observer.mu.Lock()
				defer observer.mu.Unlock()
				observer.observations.Turns++
				observer.active = true
				return nil
			},
		})
		if err != nil {
			return observer.failure(err)
		}
		if err := observeSDKHistoryTurn(ctx, observer, continued, func(run codexsdk.ResumedThreadRun) codexsdk.ThreadRunResult { return run.Run }); err != nil {
			return err
		}
		observer.mu.Lock()
		observer.observations.Stage = "history_recalled"
		observer.mu.Unlock()
		return nil
	})
	return settleSDKHistoryEvidence(evidence, err), err
}

// Evidence is copied before the SDK cleanup attempt. A late callback or cleanup
// failure can still reject that run; do not leave its whole-scenario success
// stage in the returned value. Previously observed terminal/recall facts remain.
func settleSDKHistoryEvidence(evidence Evidence, cause error) Evidence {
	if cause != nil && evidence.Stage == "history_recalled" {
		evidence.Stage = "continuation_completed"
	}
	return evidence
}

func observeSDKHistoryTurn[R any](ctx context.Context, observer *sdkHistoryObserver, stream *codexsdk.Stream[R], runResult func(R) codexsdk.ThreadRunResult) error {
	defer observer.finishTurn()
	// Result provides the actual turn/start identity without Wait's extra
	// caller-cancellation snapshot of the same retained history.
	snapshot, _ := stream.Result()
	snapshotErr := stream.Err()
	run := runResult(snapshot)
	failure := func(cause error) error {
		if latest, ok := stream.Result(); ok {
			observer.recoverNotifications(runResult(latest).Notifications)
		}
		return observer.failure(cause)
	}
	if run.Turn.ID == "" && snapshotErr != nil {
		return failure(snapshotErr)
	}
	// Reconcile the public retained snapshot before trusting an inline result.
	if err := observer.reconcile(run); err != nil {
		return err
	}
	cursorCtx, cancelCursor := context.WithCancel(ctx)
	// Cursor cancellation is caller-local. The fixture's bounded CloseContext
	// owns the process, callbacks and runs, including an inline terminal that
	// this pinned SDK does not itself terminalize. Close would interrupt it.
	defer cancelCursor()
	cursorResult := make(chan error, 1)
	go func() {
		// Next consumes only the history cursor; Notification would clone each
		// event. This SDK terminalizes the cursor after dispatched callbacks.
		for stream.Next(cursorCtx) {
		}
		err := stream.Err()
		if err == nil {
			err = cursorCtx.Err()
		}
		cursorResult <- err
	}()
	fenced := false
	for {
		observer.mu.Lock()
		cause := observer.cause
		if observer.reply.completed && (fenced || observer.inlineTerminal) {
			observer.probe.evaluateReply(&observer.reply)
		}
		// A notified terminal must cross the SDK callback fence. The original
		// inline response is independently authoritative only when no terminal
		// notification has overwritten the run snapshot.
		ready := observer.reply.completed && observer.reply.recall && !observer.reply.ambiguous &&
			(observer.probe.previousID != "" || observer.observations.ReasoningItems > 0 && observer.observations.EncryptedItems > 0 && observer.observations.CompletedTools > 0) &&
			(fenced || observer.inlineTerminal)
		settledReply := observer.reply.completed && observer.reply.authoritative && (fenced || observer.inlineTerminal)
		formatValid, recalled := observer.reply.formatValid, observer.reply.recall
		observer.mu.Unlock()
		if cause != nil {
			return cause
		}
		if settledReply && !formatValid {
			return failedObservation("history_reply_invalid", "live: history reply is not a valid structured result")
		}
		if settledReply && !recalled {
			return failedObservation("history_amount_mismatch", "live: history amount did not match the price-based calculation")
		}
		if ready {
			return nil
		}
		select {
		case <-observer.changed:
		case err := <-cursorResult:
			if err == nil {
				fenced = true
				continue
			}
			var terminal *codexsdk.TurnError
			if !errors.As(err, &terminal) {
				return failure(err)
			}
		case <-ctx.Done():
			return failure(ctx.Err())
		}
	}
}
