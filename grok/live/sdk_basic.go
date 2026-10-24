package live

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"sync"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// Basic submits one text turn through the pinned SDK. Business evidence remains
// application-owned; SDK success alone does not establish a nonempty final reply.
func Basic(ctx context.Context, options Options) (Evidence, error) {
	var mu sync.Mutex
	retainedBytes := 0
	notifications := make(chan protocolv2.ServerNotification, 128)
	failures := make(chan error, 1)
	clientContexts := make(chan context.Context, 1)
	reject := func(err error) error {
		select {
		case failures <- err:
		default:
		}
		return err
	}
	handlers := sdkFixtureHandlers{
		retention: codexsdk.NotificationRetentionBudget{MaxCount: 512, MaxBytes: 8 << 20},
		failures:  failures,
		requests: func(_ context.Context, request protocolv2.ServerRequest) (codexsdk.ServerRequestResponse, error) {
			err := reject(failedObservation("unsupported_server_request", "live: unsupported server request"))
			if request.Kind() == protocolv2.ServerRequestKindItemToolCall {
				return codexsdk.DynamicToolResponse(protocolv2.DynamicToolCallResponse{
					ContentItems: []protocolv2.DynamicToolCallOutputContentItem{}, Success: false,
				}), nil
			}
			return codexsdk.ServerRequestResponse{}, err
		},
		notifications: func(callbackCtx context.Context, notification protocolv2.ServerNotification) error {
			select {
			case clientContexts <- callbackCtx:
			default:
			}
			if notification.Kind() != protocolv2.ServerNotificationKindItemCompleted && notification.Kind() != protocolv2.ServerNotificationKindTurnCompleted {
				return nil
			}
			raw, err := json.Marshal(notification)
			if err != nil {
				return reject(failedObservation("invalid_completion", "live: invalid completion evidence"))
			}
			mu.Lock()
			defer mu.Unlock()
			if retainedBytes+len(raw) > 8<<20 {
				return reject(failedObservation("early_evidence_budget_exceeded", "live: early evidence budget exceeded"))
			}
			select {
			case notifications <- notification:
				retainedBytes += len(raw)
				return nil
			default:
				return reject(failedObservation("early_evidence_budget_exceeded", "live: early evidence budget exceeded"))
			}
		},
	}
	return withSDKFixture(ctx, options, nil, handlers, func(client *codexsdk.Client, cwd string, evidence *Evidence) error {
		evidence.Threads = 1
		stream, err := client.ThreadRunner().StartStream(ctx, codexsdk.StartThreadRunRequest{
			Thread: protocolv2.ThreadStartParams{
				Model: protocolv2.Value(options.Model), ModelProvider: protocolv2.Value("grok"), CWD: protocolv2.Value(cwd),
			},
			Turn: protocolv2.TurnStartParams{
				Input: []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{
					Text: "Reply with a short confirmation that this turn completed.",
				})},
			},
			AdmitTurn: func(thread protocolv2.ThreadStartResponse, _ protocolv2.TurnStartParams) error {
				if thread.Thread.ID == "" || thread.Model != options.Model || thread.ModelProvider != "grok" || thread.Thread.ModelProvider != "grok" {
					return failedObservation("fixture_binding_failed", "live: fixture binding not established")
				}
				evidence.Bound, evidence.Stage, evidence.Turns = true, "thread_bound", 1
				return nil
			},
		})
		if err != nil {
			return sdkFailure(err)
		}
		// Wait cancellation is caller-local. Attachment has already completed;
		// this immutable typed snapshot also covers an inline turn/start terminal.
		snapshotCtx, cancelSnapshot := context.WithCancel(context.Background())
		cancelSnapshot()
		started, err := stream.Wait(snapshotCtx)
		if started.Run.Turn.ID == "" && err != nil {
			return sdkFailure(err)
		}
		if started.Run.Turn.ID == "" {
			return failedObservation("missing_turn_identity", "live: turn identity unavailable")
		}
		evidence.Stage = "turn_submitted"
		observeReply := func(entry protocolv2.ThreadItem) {
			agent, ok := entry.AsAgentMessage()
			if ok && (agent.Phase == nil || agent.Phase.Value == nil || *agent.Phase.Value == protocolv2.MessagePhaseFinalAnswer) && strings.TrimSpace(agent.Text) != "" {
				evidence.ReplyBytes = len(agent.Text)
				if !evidence.Completed {
					evidence.Stage = "reply_observed"
				}
			}
		}
		observeTurn := func(terminal protocolv2.Turn) error {
			select {
			case err := <-failures:
				return err
			default:
			}
			if terminal.Status != protocolv2.TurnStatusCompleted || terminal.Error != nil && terminal.Error.Value != nil {
				raw, _ := json.Marshal(terminal.Error)
				return failedTurn(string(terminal.Status), raw, "live: turn did not complete successfully")
			}
			evidence.Completed, evidence.Stage = true, "turn_completed"
			for _, entry := range terminal.Items {
				observeReply(entry)
			}
			return nil
		}
		observeNotification := func(notification protocolv2.ServerNotification) error {
			if completed, ok := notification.AsItemCompleted(); ok {
				if completed.Params.ThreadID == started.Start.Thread.ID && completed.Params.TurnID == started.Run.Turn.ID {
					observeReply(completed.Params.Item)
				}
			} else if completed, ok := notification.AsTurnCompleted(); ok && completed.Params.ThreadID == started.Start.Thread.ID && completed.Params.Turn.ID == started.Run.Turn.ID {
				return observeTurn(completed.Params.Turn)
			}
			return nil
		}
		observationFailure := func(cause error) error {
			select {
			case err := <-failures:
				return err
			default:
			}
			// The SDK commits typed evidence before publishing lifecycle failure.
			// Re-read originals so EOF/cancellation cannot hide a matching cause
			// merely because its application callback has not run yet.
			if latest, ok := stream.Result(); ok {
				for _, notification := range latest.Run.Notifications {
					if err := observeNotification(notification); err != nil {
						return err
					}
				}
			}
			return sdkFailure(cause)
		}
		// The SDK snapshot is updated by terminals routed by turn ID. Only an
		// untouched snapshot proves the inline response; notification terminals
		// must independently match BOTH identities below.
		hasTerminal := false
		for _, notification := range started.Run.Notifications {
			if _, ok := notification.AsTurnCompleted(); ok {
				hasTerminal = true
			}
			if err := observeNotification(notification); err != nil {
				return err
			}
		}
		if !hasTerminal && started.Run.Turn.Status != protocolv2.TurnStatusInProgress {
			if err := observeTurn(started.Run.Turn); err != nil {
				return err
			}
		}
		var clientDone <-chan struct{}
		streamFailures := make(chan error, 1)
		go func() {
			if _, err := stream.Wait(ctx); err != nil {
				streamFailures <- err
			}
		}()
		for !evidence.Completed || evidence.ReplyBytes == 0 {
			select {
			case err := <-failures:
				return err
			default:
			}
			select {
			case notification := <-notifications:
				raw, _ := json.Marshal(notification)
				mu.Lock()
				retainedBytes -= len(raw)
				mu.Unlock()
				if err := observeNotification(notification); err != nil {
					return err
				}
			case callbackCtx := <-clientContexts:
				clientDone = callbackCtx.Done()
			case err := <-failures:
				return err
			case err := <-streamFailures:
				// Exact-run errors alone cannot attribute a failed terminal to
				// our thread. Only the original correlated notification can.
				var terminal *codexsdk.TurnError
				if errors.As(err, &terminal) {
					continue
				}
				return observationFailure(err)
			case <-clientDone:
				return observationFailure(codexsdk.ErrClientClosed)
			case <-ctx.Done():
				return observationFailure(ctx.Err())
			}
		}
		select {
		case err := <-failures:
			return err
		default:
		}
		evidence.Stage = "final_reply"
		return nil
	})
}
