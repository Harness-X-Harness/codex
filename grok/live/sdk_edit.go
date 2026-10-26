package live

import (
	"context"
	"fmt"
	"path/filepath"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func sdkStructuredEdit(ctx context.Context, options Options, fixture editFixture) (StructuredEditEvidence, error) {
	observer := newSDKEditObserver(&fixture)
	var evidence StructuredEditEvidence
	checkBytes := func(expected string) error {
		data, err := readEditFixture(fixture.path)
		if err != nil {
			return err
		}
		evidence.AfterSHA256 = editDigest(data)
		evidence.BytesMatch = string(data) == expected
		if !evidence.BytesMatch {
			return failedObservation("edit_bytes_mismatch", "live: edit fixture bytes did not match")
		}
		return nil
	}
	handlers := sdkFixtureHandlers{
		requests: observer.request, notifications: observer.typedNotification, failures: observer.failures,
		retention: codexsdk.NotificationRetentionBudget{MaxCount: 4096, MaxBytes: 8 << 20},
		afterClose: func(_ string, base *Evidence, cause error) error {
			snapshot, observationErr := observer.snapshot()
			base.Turns, base.FirstCompleted, base.FirstReplyBytes = snapshot.Turns, snapshot.FirstCompleted, snapshot.FirstReplyBytes
			base.ReplyBytes = snapshot.ReplyBytes
			if snapshot.Stage != "" {
				base.Stage = snapshot.Stage
			}
			evidence.TurnCompleted = snapshot.TurnCompleted
			evidence.FileChanges, evidence.Approvals = snapshot.FileChanges, snapshot.Approvals
			if observationErr != nil {
				cause = observationErr
			}
			if cause == nil {
				cause = observer.requireTurnProof()
			}
			if cause == nil {
				cause = checkBytes(fixture.expected)
			}
			base.Completed, evidence.BytesMatch, evidence.ContinuationUnchanged = false, false, false
			if cause != nil {
				return observer.failure(cause)
			}
			base.Completed, evidence.BytesMatch, evidence.ContinuationUnchanged = true, true, fixture.continuation
			base.Stage = "edit_observed"
			if fixture.continuation {
				base.Stage = "edit_continuation_observed"
			}
			return nil
		},
	}
	base, err := withSDKFixture(ctx, options, &fixture, handlers, func(client *codexsdk.Client, cwd string, base *Evidence) error {
		seed, err := readEditFixture(fixture.path)
		if err != nil {
			return err
		}
		evidence.BeforeSHA256 = editDigest(seed)
		if string(seed) != fixture.seed {
			return failedObservation("edit_bytes_mismatch", "live: edit fixture seed did not match")
		}
		policy := protocolv2.NewAskForApprovalNever()
		if fixture.decline {
			policy = protocolv2.NewAskForApprovalUntrusted()
		}
		fallback := false
		base.Threads = 1
		started, err := client.Threads().Start(ctx, protocolv2.ThreadStartParams{
			Model: protocolv2.Value(options.Model), ModelProvider: protocolv2.Value("grok"), CWD: protocolv2.Value(cwd),
			AllowProviderModelFallback: &fallback, ApprovalPolicy: protocolv2.Value(policy),
		})
		if err != nil {
			return sdkEditCause(ctx, observer, err)
		}
		sandbox, writable := started.Sandbox.AsWorkspaceWrite()
		if started.Model != options.Model || started.ModelProvider != "grok" || started.Thread.ModelProvider != "grok" ||
			started.Thread.ID == "" || started.CWD != cwd || started.Thread.CWD != cwd || started.ApprovalPolicy.Kind() != policy.Kind() ||
			!writable || sandbox.NetworkAccess != nil && *sandbox.NetworkAccess {
			return failedObservation("fixture_binding_failed", "live: fixture binding not established")
		}
		if sandbox.WritableRoots != nil {
			for _, root := range *sandbox.WritableRoots {
				if filepath.Clean(root) != cwd {
					return failedObservation("fixture_binding_failed", "live: fixture writable roots not bound")
				}
			}
		}
		if err := observer.bindThread(started.Thread.ID); err != nil {
			return err
		}
		base.Bound, base.Stage = true, "thread_bound"
		prompt := fmt.Sprintf("In %s, replace %s with %s, preserving all other file content. If approval is declined, leave the file unchanged.", editFile, editOld, editNew)
		if fixture.replaceAll {
			prompt += " Replace both occurrences."
		}
		prompts := []string{prompt}
		if fixture.continuation {
			prompts = append(prompts, "Check that the requested edit is complete and confirm the result. Leave the file content unchanged.")
		}
		for _, prompt := range prompts {
			evidence.BytesMatch, evidence.AfterSHA256 = false, ""
			if err := observer.beginTurn(); err != nil {
				return err
			}
			turn, err := client.Turns().Start(ctx, protocolv2.TurnStartParams{
				ThreadID: started.Thread.ID, Input: []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{Text: prompt})},
			})
			if err != nil {
				return sdkEditCause(ctx, observer, err)
			}
			if err := observer.reconcile(turn.Turn); err != nil {
				return err
			}
			if err := waitSDKEditTurn(ctx, observer); err != nil {
				return err
			}
			includeTurns := true
			settled, err := client.Threads().Read(ctx, protocolv2.ThreadReadParams{ThreadID: started.Thread.ID, IncludeTurns: &includeTurns})
			if err != nil {
				return sdkEditCause(ctx, observer, err)
			}
			// thread/read returns the platform's own cwd spelling. The fixture bytes below are the effect readback.
			if settled.Thread.ModelProvider != "grok" || settled.Thread.ID == "" {
				return failedObservation("fixture_binding_failed", "live: settled fixture binding not established")
			}
			if err := observer.validateSettled(settled.Thread.ID, settled.Thread.Turns); err != nil {
				return err
			}
			if err := observer.requireTurnProof(); err != nil {
				return err
			}
			if err := checkBytes(fixture.expected); err != nil {
				return err
			}
		}
		return nil
	})
	evidence.Evidence = base
	return evidence, err
}

func sdkEditCause(ctx context.Context, observer *sdkEditObserver, err error) error {
	if ctx.Err() != nil {
		return observer.failure(ctx.Err())
	}
	return observer.failure(err)
}

func waitSDKEditTurn(ctx context.Context, observer *sdkEditObserver) error {
	for {
		observer.mu.Lock()
		cause := observer.cause
		ledger := observer.ledgers[len(observer.ledgers)-1]
		// A callback terminal is the serial notification prefix. An inline
		// start terminal still has to wait for its public reply and any denial evidence.
		ready := ledger.callbackTerminal || ledger.inlineTerminal && ledger.proof() == nil
		clientDone := observer.clientDone
		observer.mu.Unlock()
		if cause != nil {
			return cause
		}
		if ready {
			return observer.requireTurnProof()
		}
		select {
		case <-observer.changed:
		case <-clientDone:
			return observer.failure(codexsdk.ErrClientClosed)
		case <-ctx.Done():
			return observer.failure(ctx.Err())
		}
	}
}
