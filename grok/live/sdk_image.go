package live

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"time"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func sdkImageGenerationEdit(ctx context.Context, options Options) (ImageEvidence, error) {
	evidence := ImageEvidence{}
	probe := &imageProbe{
		ctx: ctx, evidence: &evidence, calls: map[string]*imageCall{},
		changed: make(chan struct{}, 1),
	}
	handlers := sdkFixtureHandlers{
		requests: probe.request, notifications: probe.typedNotification, failures: make(chan error, 1),
		retention: codexsdk.NotificationRetentionBudget{MaxCount: 8192, MaxBytes: 256 << 20},
		// The business counter rejects the 4097th frame. The SDK default queue
		// of 64 closes the client before that frame is delivered.
		queue: 8192,
		raw: &codexsdk.RawNotificationSubscription{
			Methods:                        []string{codexsdk.RawResponseItemCompletedExtension},
			EnableRawResponseItemExtension: true, Handler: probe.rawNotification,
		},
	}
	base, err := withSDKFixture(ctx, options, nil, handlers, func(client *codexsdk.Client, cwd string, base *Evidence) error {
		probe.closed = func() error {
			closeCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			_, closeErr := client.CloseContext(closeCtx)
			return closeErr
		}
		probe.base = base
		probe.root = filepath.Join(filepath.Dir(cwd), "generated_images")
		policy := protocolv2.NewAskForApprovalNever()
		experimental := true
		base.Threads = 1
		started, err := client.Threads().Start(ctx, protocolv2.ThreadStartParams{
			Model: protocolv2.Value(options.Model), ModelProvider: protocolv2.Value("grok"), CWD: protocolv2.Value(cwd),
			ExperimentalRawEvents: &experimental, ApprovalPolicy: protocolv2.Value(policy),
		})
		if err != nil {
			return imageTransport(ctx, err)
		}
		if _, readOnly := started.Sandbox.AsReadOnly(); !readOnly || started.Model != options.Model || started.ModelProvider != "grok" ||
			started.Thread.ModelProvider != "grok" || started.Thread.ID == "" || started.CWD != cwd || started.Thread.CWD != cwd ||
			started.ApprovalPolicy.Kind() != policy.Kind() || len(started.Thread.Turns) != 0 {
			return failedObservation("fixture_binding_failed", "live: fixture binding not established")
		}
		probe.threadID = started.Thread.ID
		base.Bound, base.Stage = true, "thread_bound"
		prompts := []string{
			"Generate an image of a blue circle on a plain white background.",
			"Edit the image you just generated so the circle is green while keeping the plain white background.",
		}
		for _, prompt := range prompts {
			probe.mu.Lock()
			err = probe.prepare()
			probe.mu.Unlock()
			if err != nil {
				return err
			}
			turn, err := client.Turns().Start(ctx, protocolv2.TurnStartParams{
				ThreadID: probe.threadID, Input: []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{Text: prompt})},
			})
			if err != nil {
				return imageTransport(ctx, err)
			}
			raw, err := json.Marshal(turn.Turn)
			var decoded imageTurn
			if err != nil || json.Unmarshal(raw, &decoded) != nil {
				return errors.New("live: invalid image evidence")
			}
			probe.mu.Lock()
			err = probe.acceptStarted(decoded)
			probe.mu.Unlock()
			if err != nil {
				return err
			}
			if err := probe.wait(ctx); err != nil {
				return err
			}
			includeTurns := true
			settled, err := client.Threads().Read(ctx, protocolv2.ThreadReadParams{ThreadID: probe.threadID, IncludeTurns: &includeTurns})
			if err != nil {
				return imageTransport(ctx, err)
			}
			encoded, err := json.Marshal(settled.Thread)
			var thread struct {
				ID    string      `json:"id"`
				Turns []imageTurn `json:"turns"`
			}
			if err != nil || json.Unmarshal(encoded, &thread) != nil {
				return errors.New("live: invalid image evidence")
			}
			probe.mu.Lock()
			err = probe.settle(thread.ID, thread.Turns)
			probe.mu.Unlock()
			if err != nil {
				return err
			}
			if prompt == prompts[0] {
				probe.mu.Lock()
				newest, turnID := probe.newest, probe.turnID
				if newest == nil {
					probe.mu.Unlock()
					return errors.New("live: completed canonical image unavailable")
				}
				probe.prior, probe.previousID = newest, turnID
				evidence.FirstImageSHA256, evidence.FirstHistorySHA256 = newest.digest, newest.output
				probe.mu.Unlock()
			}
		}
		probe.mu.Lock()
		newest, prior := probe.newest, probe.prior
		probe.mu.Unlock()
		if !evidence.HistorySelected || newest == nil || prior == nil || !newest.fromPrior || newest.digest == prior.digest {
			return errors.New("live: distinct history-derived edit unavailable")
		}
		if _, err := inspectImage(probe.root, prior.path, prior.digest); err != nil {
			return err
		}
		if ctx.Err() != nil {
			return errors.New("live: image observation deadline exceeded")
		}
		evidence.EditedImageSHA256, evidence.EditedHistorySHA256 = newest.digest, newest.output
		base.Completed, base.Stage = true, "image_edit_verified"
		return nil
	})
	evidence.Evidence = base
	return evidence, err
}

func imageTransport(ctx context.Context, err error) error {
	if ctx.Err() != nil {
		return errors.New("live: image observation deadline exceeded")
	}
	if frameTooLarge(err) {
		return errors.New("live: protocol frame budget exceeded")
	}
	var protocol *codexsdk.ProtocolError
	if errors.As(err, &protocol) {
		return errors.New("live: protocol request failed")
	}
	if probeDone(err) {
		return errors.New("live: protocol ended before proof completion")
	}
	return failedObservation("sdk_observation_failed", "live: SDK observation did not complete")
}

func frameTooLarge(err error) bool {
	// The SDK frame sentinel is unexported. Its text is a static class plus a
	// byte count and digest, never the rejected frame body.
	return err != nil && strings.Contains(err.Error(), "JSON-RPC frame too large")
}

func probeDone(err error) bool {
	return errors.Is(err, codexsdk.ErrClientClosed) || errors.Is(err, context.Canceled)
}

func (probe *imageProbe) wait(ctx context.Context) error {
	for {
		probe.mu.Lock()
		cause, done, clientDone := probe.cause, probe.completed, probe.clientDone
		probe.mu.Unlock()
		if cause != nil {
			return cause
		}
		if done {
			return nil
		}
		select {
		case <-probe.changed:
		case <-clientDone:
			probe.mu.Lock()
			cause, closed := probe.cause, probe.closed
			probe.mu.Unlock()
			if cause != nil {
				return cause
			}
			if closed != nil && frameTooLarge(closed()) {
				return errors.New("live: protocol frame budget exceeded")
			}
			return errors.New("live: protocol ended before proof completion")
		case <-ctx.Done():
			return errors.New("live: image observation deadline exceeded")
		}
	}
}

func (probe *imageProbe) request(ctx context.Context, _ protocolv2.ServerRequest) (codexsdk.ServerRequestResponse, error) {
	probe.mu.Lock()
	defer probe.mu.Unlock()
	probe.noteCallback(ctx)
	probe.cause = errors.New("live: unsupported server request")
	probe.signal()
	return codexsdk.ServerRequestResponse{}, nil
}

func (probe *imageProbe) rawNotification(ctx context.Context, notification codexsdk.RawNotification) error {
	if notification.Method != codexsdk.RawResponseItemCompletedExtension {
		return nil
	}
	raw, err := notification.Envelope.MarshalJSON()
	return probe.observeFrame(ctx, notification.Method, raw, err)
}

func (probe *imageProbe) typedNotification(ctx context.Context, notification protocolv2.ServerNotification) error {
	method := "item/completed"
	if _, ok := notification.AsItemStarted(); ok {
		method = "item/started"
	} else if _, ok := notification.AsTurnCompleted(); ok {
		method = "turn/completed"
	} else if _, ok := notification.AsItemCompleted(); !ok {
		return nil
	}
	raw, err := json.Marshal(notification)
	return probe.observeFrame(ctx, method, raw, err)
}

func (probe *imageProbe) observeFrame(ctx context.Context, method string, raw []byte, marshalErr error) error {
	probe.mu.Lock()
	defer probe.mu.Unlock()
	probe.noteCallback(ctx)
	var envelope struct {
		Params json.RawMessage `json:"params"`
	}
	if marshalErr != nil || json.Unmarshal(raw, &envelope) != nil || len(envelope.Params) == 0 {
		probe.cause = errors.New("live: invalid image evidence")
		probe.signal()
		return nil
	}
	message := frame{Method: method, Params: envelope.Params, size: len(raw)}
	if probe.turnID == "" {
		probe.pending = append(probe.pending, message)
		probe.signal()
		return nil
	}
	probe.ingest(message)
	return nil
}

func (probe *imageProbe) noteCallback(ctx context.Context) {
	if probe.clientDone == nil && ctx != nil {
		probe.clientDone = ctx.Done()
	}
}

func (probe *imageProbe) signal() {
	select {
	case probe.changed <- struct{}{}:
	default:
	}
}
