package live

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"

	"github.com/Harness-X-Harness/codex/grok/internal/integrationfixture"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

const historyTool = "apple_price_lookup"

const historyOutputSchema = `{"type":"object","properties":{"amount":{"type":["number","null"]}},"required":["amount"],"additionalProperties":false}`

// This bounds replay evidence, not product tool calls or retries.
const historyAssistantLimit = 4096

// ReasoningHistory is the executable two-turn shop example.
// Both schemas are submitted explicitly; assertions never resubmit work.
func ReasoningHistory(ctx context.Context, options Options) (Evidence, error) {
	schema, schemaErr := protocolv2.OutputSchemaFromJSON([]byte(historyOutputSchema))
	if schemaErr != nil {
		return Evidence{}, failedObservation("history_schema_invalid", "live: history output schema unavailable")
	}
	observer := newSDKHistoryObserver()
	observer.diagnostics = options.Diagnostics
	handlers := sdkFixtureHandlers{
		requests: observer.request, notifications: observer.typedNotification, failures: observer.failures,
		retention: codexsdk.NotificationRetentionBudget{MaxCount: 65536, MaxBytes: 64 << 20},
		raw: &codexsdk.RawNotificationSubscription{
			Methods:                        []string{codexsdk.RawResponseItemCompletedExtension},
			EnableRawResponseItemExtension: true, Handler: observer.notification,
		},
	}
	evidence, err := withSDKFixture(ctx, options, nil, handlers, func(client *codexsdk.Client, cwd string, evidence *Evidence, closeSDK func() error) error {
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
			pending := evidence.CleanupPending
			*evidence = observer.observations
			evidence.CleanupPending = pending
		}()
		turn := protocolv2.TurnStartParams{
			OutputSchema: &schema,
			Input: []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{
				Text: "Use the shop's apple_price_lookup tool as the authoritative source for its current apple price, then calculate how much two kilograms cost in yuan.",
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
				observer.threadID = thread.Thread.ID
				observer.observations.Bound, observer.observations.Stage = true, "thread_bound"
				observer.observations.Turns++
				observer.active = true
				return nil
			},
		})
		if err != nil {
			return observer.failure(err)
		}
		firstCursor, cancelFirstCursor, turnErr := observeSDKHistoryTurn(ctx, observer, stream, func(run codexsdk.StartedThreadRun) codexsdk.ThreadRunResult { return run.Run })
		defer cancelFirstCursor()
		// The shop result is an assertion of this ordinary program, not a
		// transport-ready predicate. Record partial facts even on failure.
		amountErr := observer.assertAmount(2)
		if turnErr == nil {
			turnErr = amountErr
		}
		if turnErr == nil {
			turnErr = awaitFirstHistoryFacts(ctx, observer, stream, func(run codexsdk.StartedThreadRun) codexsdk.ThreadRunResult { return run.Run }, firstCursor)
		}
		// Inline completion can precede independently delivered evidence.
		// Recheck the final selected response after that evidence settles.
		turnErr = observer.settleTurn(2, turnErr)
		cancelFirstCursor()
		if turnErr != nil {
			return turnErr
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
		_, cancelSecondCursor, turnErr := observeSDKHistoryTurn(ctx, observer, continued, func(run codexsdk.ResumedThreadRun) codexsdk.ThreadRunResult { return run.Run })
		defer cancelSecondCursor()
		turnErr = observer.settleTurn(3, turnErr)
		cancelSecondCursor()
		if turnErr != nil {
			return turnErr
		}
		if err := closeSDK(); err != nil {
			return err
		}
		// Joined callbacks may revoke the result; no success is emitted first.
		if err := observer.failure(nil); err != nil {
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

func observeSDKHistoryTurn[R any](ctx context.Context, observer *sdkHistoryObserver, stream *codexsdk.Stream[R], runResult func(R) codexsdk.ThreadRunResult) (cursorResult <-chan error, cancelCursor context.CancelFunc, resultErr error) {
	cancelCursor = func() {}
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
		return cursorResult, cancelCursor, failure(snapshotErr)
	}
	// Reconcile the public retained snapshot before trusting an inline result.
	if err := observer.reconcile(run); err != nil {
		return cursorResult, cancelCursor, err
	}
	var cursorCtx context.Context
	cursorCtx, cancelCursor = context.WithCancel(ctx)
	// Cursor cancellation is caller-local. The fixture's bounded CloseContext
	// owns the process, callbacks and runs, including an inline terminal that
	// this pinned SDK does not itself terminalize. Close would interrupt it.
	results := make(chan error, 1)
	cursorResult = results
	go func() {
		// Next consumes only the history cursor; Notification would clone each
		// event. This SDK terminalizes the cursor after dispatched callbacks.
		for stream.Next(cursorCtx) {
		}
		err := stream.Err()
		if err == nil {
			err = cursorCtx.Err()
		}
		results <- err
	}()
	fenced := false
	for {
		observer.mu.Lock()
		cause := observer.cause
		// Collection owns identity and callback ordering, never the expected
		// shop total. Conflicting provisional identities stay unproven.
		if observer.reply.completed && (fenced || observer.inlineTerminal) {
			observer.evaluateReply(&observer.reply)
		}
		// An empty terminal may precede complete item notifications. Retain
		// that compatibility without letting the expected amount drive waiting.
		ready := observer.reply.completed && !observer.reply.ambiguous &&
			(observer.reply.authoritative || observer.reply.formatValid) && (fenced || observer.inlineTerminal)
		observer.mu.Unlock()
		if cause != nil {
			return cursorResult, cancelCursor, cause
		}
		if ready {
			return cursorResult, cancelCursor, nil
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
				return cursorResult, cancelCursor, failure(err)
			}
		case <-ctx.Done():
			return cursorResult, cancelCursor, failure(ctx.Err())
		}
	}
}

// assertAmount evaluates the complete selected reply. The caller supplies the
// task quantity explicitly; neither callback state nor transport chooses it.
func (observer *sdkHistoryObserver) assertAmount(kilograms int) error {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	observer.evaluateReply(&observer.reply)
	reply := &observer.reply
	reply.recall = historyAmountMatches(reply, observer.unitPrice, kilograms)
	if !reply.recall {
		return observer.replyFailure()
	}
	return nil
}

// A raw encrypted item may itself establish reasoning; no second typed
// reasoning representation is required. Keep this inline-compatibility shim:
// the pinned SDK has no callback flush that keeps the same process resumable.
func awaitFirstHistoryFacts[R any](ctx context.Context, observer *sdkHistoryObserver, stream *codexsdk.Stream[R], runResult func(R) codexsdk.ThreadRunResult, cursorErrors <-chan error) error {
	failure := func(cause error) error {
		if latest, ok := stream.Result(); ok {
			observer.recoverNotifications(runResult(latest).Notifications)
		}
		return observer.failure(cause)
	}
	for {
		observer.mu.Lock()
		cause := observer.cause
		ready := !observer.reply.ambiguous && observer.observations.ReasoningItems > 0 && observer.observations.EncryptedItems > 0 && observer.observations.CompletedTools > 0
		observer.mu.Unlock()
		if cause != nil {
			return cause
		}
		if ready {
			return nil
		}
		select {
		case <-observer.changed:
		case err := <-cursorErrors:
			if err == nil {
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

// Freeze the selected response and its business verdict under the same lock.
// Callbacks after this point may contradict, but cannot add positive credit.
func (observer *sdkHistoryObserver) settleTurn(kilograms int, prior error) error {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	observer.evaluateReply(&observer.reply)
	observer.reply.recall = historyAmountMatches(&observer.reply, observer.unitPrice, kilograms)
	observer.finishTurnLocked()
	if prior != nil {
		return prior
	}
	if observer.cause != nil {
		return observer.cause
	}
	if !observer.reply.completed || observer.reply.ambiguous {
		return failedObservation("assistant_identity_conflict", "live: turn proof did not settle unambiguously")
	}
	if !observer.reply.recall {
		return observer.replyFailure()
	}
	return nil
}

func historyAmountMatches(reply *historyReply, unitPrice, kilograms int) bool {
	return reply.formatValid && reply.amount != nil && unitPrice > 0 && *reply.amount == float64(kilograms*unitPrice)
}

func (observer *sdkHistoryObserver) request(_ context.Context, request protocolv2.ServerRequest) (codexsdk.ServerRequestResponse, error) {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	call, ok := request.AsItemToolCall()
	if !ok {
		cause := observer.reject(failedObservation("unsupported_server_request", "live: unsupported server request"))
		// Known approval requests get a typed denial even if normal fixture
		// shutdown begins before the SDK writes the callback response.
		if _, command := request.AsItemCommandExecutionRequestApproval(); command {
			return codexsdk.CommandExecutionApprovalResponse(protocolv2.CommandExecutionRequestApprovalResponse{
				Decision: protocolv2.NewCommandExecutionApprovalDecisionDecline(),
			}), nil
		}
		if _, file := request.AsItemFileChangeRequestApproval(); file {
			return codexsdk.FileChangeApprovalResponse(protocolv2.FileChangeRequestApprovalResponse{
				Decision: protocolv2.FileChangeApprovalDecisionDecline,
			}), nil
		}
		return codexsdk.ServerRequestResponse{}, cause
	}
	params := call.Params
	observer.diagnostics.Record("history_tool_input", map[string]any{"turn_id": params.TurnID, "call_id": params.CallID, "tool": params.Tool, "arguments": params.Arguments})
	arguments, object := params.Arguments.AsObject()
	text, success := "Price lookup is unavailable during continuation.", false
	valid := observer.active && params.ThreadID == observer.threadID && params.TurnID != "" && params.TurnID != observer.previousID &&
		(observer.turnID == "" || params.TurnID == observer.turnID) && params.CallID != "" &&
		(params.Namespace == nil || params.Namespace.Value == nil) && params.Tool == historyTool && object && len(arguments) == 0
	if !valid || observer.cause != nil {
		observer.reject(failedObservation("history_tool_request_unattributed", "live: history tool request not authorized"))
	} else if observer.previousID != "" {
		observer.turnID = params.TurnID
		observer.observations.DeniedToolCalls++
	} else {
		identity := sha256.Sum256([]byte(params.CallID))
		_, seen := observer.acceptedCalls[identity]
		if !seen && len(observer.acceptedCalls) == historyToolIdentityLimit {
			observer.reject(failedObservation("history_tool_identity_budget_exceeded", "live: history tool identity evidence budget exceeded"))
		} else {
			if observer.unitPrice == 0 {
				var sample [1]byte
				if _, err := rand.Read(sample[:]); err != nil {
					return codexsdk.ServerRequestResponse{}, observer.reject(failedObservation("history_price_unavailable", "live: tool price unavailable"))
				}
				observer.unitPrice = 5 + int(sample[0]%15)
				observer.toolResult = fmt.Sprintf(`{"unit_price_yuan":%d}`, observer.unitPrice)
			}
			observer.turnID = params.TurnID
			observer.acceptedCalls[identity] = struct{}{}
			observer.observations.ToolCalls++
			text, success = observer.toolResult, true
		}
	}
	observer.diagnostics.Record("history_tool_output", map[string]any{"turn_id": params.TurnID, "call_id": params.CallID, "success": success, "text": text})
	return codexsdk.DynamicToolResponse(protocolv2.DynamicToolCallResponse{
		ContentItems: []protocolv2.DynamicToolCallOutputContentItem{protocolv2.NewDynamicToolCallOutputContentItemInputText(protocolv2.DynamicToolCallOutputContentItemInputText{Text: text})}, Success: success,
	}), nil
}

// Classify the failed proof, not an unobserved backend cause. The shop price
// exists only after an accepted callback; without it there is no expected amount.
func (observer *sdkHistoryObserver) replyFailure() error {
	code, message := "history_reply_not_recalled", "live: continuation did not recall the checked price"
	if !observer.reply.formatValid {
		code, message = "history_reply_invalid", "live: final reply did not match the amount schema"
	} else if observer.previousID == "" {
		if observer.unitPrice == 0 || len(observer.acceptedCalls) == 0 {
			code, message = "history_lookup_not_observed", "live: no accepted shop price lookup was observed"
		} else {
			code, message = "history_amount_mismatch", "live: final amount did not match the checked shop price"
		}
	}
	observer.diagnostics.Record("history_outcome", map[string]any{
		"code": code, "stage": observer.observations.Stage,
		"lookup_accepted":   len(observer.acceptedCalls) > 0,
		"price_initialized": observer.unitPrice > 0,
		"format_valid":      observer.reply.formatValid,
		"continuation":      observer.previousID != "",
	})
	return failedObservation(code, message)
}
