package live

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"sync"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func sdkImageGenerationEdit(ctx context.Context, options Options) (ImageEvidence, error) {
	evidence := ImageEvidence{}
	server := &sdkProductObserver{ctx: ctx, changed: make(chan struct{}, 1), diagnostics: options.Diagnostics}
	handlers := sdkFixtureHandlers{requests: server.request, notifications: server.typedNotification, afterClose: server.afterClose,
		retention: codexsdk.NotificationRetentionBudget{MaxCount: 8192, MaxBytes: 256 << 20}, queue: 8192}
	base, err := withSDKFixture(ctx, options, nil, handlers, func(client *codexsdk.Client, cwd string, base *Evidence) error {
		server.client = client
		root := filepath.Join(filepath.Dir(cwd), "generated_images")
		type identity struct{ thread, turn string }
		var submitted []identity
		var terminalMu sync.Mutex
		var terminals []frame
		server.setNotification(func(message frame) error {
			if message.Method != "turn/completed" {
				return nil
			}
			terminalMu.Lock()
			defer terminalMu.Unlock()
			if len(terminals) >= 128 {
				return failedObservation("terminal_budget_exceeded", "live: terminal evidence budget exceeded")
			}
			terminals = append(terminals, message)
			return nil
		})
		server.finalize = func() error {
			terminalMu.Lock()
			defer terminalMu.Unlock()
			for _, message := range terminals {
				for _, id := range submitted {
					if _, err := productTerminal(message, id.thread, id.turn); err != nil {
						return err
					}
				}
			}
			return nil
		}
		startedThreads := map[string]bool{}
		start := func() (string, error) {
			fallback := false
			started, err := client.Threads().Start(ctx, protocolv2.ThreadStartParams{Model: protocolv2.Value(options.Model), ModelProvider: protocolv2.Value("grok"), CWD: protocolv2.Value(cwd), AllowProviderModelFallback: &fallback, ApprovalPolicy: protocolv2.Value(protocolv2.NewAskForApprovalNever())})
			if err != nil {
				return "", imageTransport(ctx, err)
			}
			if started.Thread.ID == "" || started.Model != options.Model || started.ModelProvider != "grok" || started.Thread.ModelProvider != "grok" || started.CWD != cwd || started.Thread.CWD != cwd || len(started.Thread.Turns) != 0 {
				return "", failedObservation("fixture_binding_failed", "live: fixture binding not established")
			}
			if startedThreads[started.Thread.ID] {
				return "", failedObservation("image_recognition_not_independent", "live: image recognition conversation is not independent")
			}
			startedThreads[started.Thread.ID] = true
			base.Threads++
			base.Bound = true
			return started.Thread.ID, nil
		}
		run := func(id string, input []protocolv2.UserInput) (productTurn, error) {
			base.Completed = false
			_, turn, err := observeProductCompletedTurn(server, id, base, "", input)
			if turn.ID != "" {
				for _, prior := range submitted {
					if prior == (identity{id, turn.ID}) {
						return turn, failedObservation("image_turn_reused", "live: image continuation did not establish a new turn")
					}
				}
				submitted = append(submitted, identity{id, turn.ID})
			}
			return turn, err
		}
		generationThread, err := start()
		if err != nil {
			return err
		}
		prompts := []string{"Draw a clear picture of a dog on a plain background.", "Add a red hat to the dog in the image you just generated, keeping the dog and background."}
		for index, prompt := range prompts {
			phase := []string{"generation", "edit"}[index]
			options.Diagnostics.Record("image_phase", map[string]any{"phase": phase, "status": "submitted", "prompt": prompt})
			evidence.TurnCompleted = false
			turn, err := run(generationThread, []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{Text: prompt})})
			recordImageTurn(options.Diagnostics, phase, turn)
			options.Diagnostics.Record("image_phase", map[string]any{"phase": phase, "status": "returned", "completed": err == nil})
			if err != nil {
				return err
			}
			evidence.TurnCompleted = true
			if index == 0 {
				base.FirstCompleted = true
			}
			result, err := completedImage(turn, root)
			if err != nil {
				return err
			}
			evidence.Images++
			evidence.Artifacts++
			if index == 0 {
				evidence.FirstImageSHA256 = result.digest
			} else {
				evidence.EditedImageSHA256 = result.digest
			}
			// The recognition conversation sees only the actual pixels and a neutral
			// question. Neither the generation request nor the expected answer is sent.
			recognitionThread, err := start()
			if err != nil {
				return err
			}
			if recognitionThread == generationThread {
				return failedObservation("image_recognition_not_independent", "live: image recognition conversation is not independent")
			}
			recognitionPrompt := "Describe the animal and anything it is wearing in this picture. Answer in English."
			options.Diagnostics.Record("image_phase", map[string]any{"phase": phase + "_recognition", "status": "submitted", "prompt": recognitionPrompt, "input_sha256": result.digest})
			recognized, err := run(recognitionThread, []protocolv2.UserInput{imageInput(result), protocolv2.NewUserInputText(protocolv2.UserInputText{Text: recognitionPrompt})})
			options.Diagnostics.Record("image_phase", map[string]any{"phase": phase + "_recognition", "status": "returned", "completed": err == nil})
			if err != nil {
				return err
			}
			base.Stage = "image_" + phase + "_recognition_completed"
			reply, err := finalProductReply(recognized)
			options.Diagnostics.Record("image_recognition", map[string]any{"phase": phase, "reply": reply, "aggregate_available": err == nil})
			if err != nil {
				return err
			}
			base.ReplyBytes = len(reply)
			accepted := imageRecognized(reply, index == 1)
			options.Diagnostics.Record("image_verdict", map[string]any{"phase": phase, "accepted": accepted})
			if !accepted {
				return failedObservation("image_visual_result_mismatch", "live: requested image content not established")
			}
			if index == 0 {
				evidence.GenerationRecognized = true
				base.Stage = "image_generation_verified"
			} else {
				evidence.EditRecognized = true
			}
		}
		base.Completed, base.Stage = true, "image_edit_verified"
		return nil
	})
	evidence.Evidence = base
	return evidence, err
}

func imageTransport(ctx context.Context, err error) error {
	if ctx.Err() != nil {
		return failedObservation("image_observation_context_done", "live: image observation deadline exceeded")
	}
	if frameTooLarge(err) {
		return failedObservation("protocol_frame_budget_exceeded", "live: protocol frame budget exceeded")
	}
	var protocol *codexsdk.ProtocolError
	if errors.As(err, &protocol) {
		return failedObservation("protocol_request_failed", "live: protocol request failed")
	}
	if probeDone(err) {
		return failedObservation("protocol_ended_before_completion", "live: protocol ended before proof completion")
	}
	return failedObservation("sdk_observation_failed", "live: SDK observation did not complete")
}
func frameTooLarge(err error) bool {
	return err != nil && strings.Contains(err.Error(), "JSON-RPC frame too large")
}
func probeDone(err error) bool {
	return errors.Is(err, codexsdk.ErrClientClosed) || errors.Is(err, context.Canceled)
}
