package live

import (
	"bytes"
	"context"
	"encoding/base64"
	"image"
	"image/color"
	"image/draw"
	"image/png"
	"strings"
	"sync"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

const attachedImageFirstPrompt = "Look at the attached picture. What color and shape is the object on the left?"
const attachedImageNextPrompt = "In the picture I attached earlier, what color and shape is the object on the right?"

// attachedImageURL carries the answers only in pixels, not a filename or prompt.
// Conversation text is usable context; this does not claim pixel-only recall.
func attachedImageURL() (string, error) {
	canvas := image.NewRGBA(image.Rect(0, 0, 480, 240))
	draw.Draw(canvas, canvas.Bounds(), image.NewUniform(color.White), image.Point{}, draw.Src)
	for y := 40; y < 200; y++ {
		for x := 40; x < 200; x++ {
			if (x-120)*(x-120)+(y-120)*(y-120) <= 80*80 {
				canvas.Set(x, y, color.RGBA{R: 255, A: 255})
			}
		}
	}
	draw.Draw(canvas, image.Rect(280, 40, 440, 200), image.NewUniform(color.RGBA{B: 255, A: 255}), image.Point{}, draw.Src)
	var data bytes.Buffer
	if err := png.Encode(&data, canvas); err != nil {
		return "", err
	}
	return "data:image/png;base64," + base64.StdEncoding.EncodeToString(data.Bytes()), nil
}

const attachedImageSchemaJSON = `{"type":"object","properties":{"color":{"type":["string","null"]},"shape":{"type":["string","null"]}},"required":["color","shape"],"additionalProperties":false}`

type attachedImageProperties struct {
	Color *string `json:"color"`
	Shape *string `json:"shape"`
}

// These are actual fixture facts, not a prose classifier. Null remains unknown.
func (answer attachedImageProperties) matches(color, shape string) bool {
	if answer.Color == nil || answer.Shape == nil {
		return false
	}
	observedShape := strings.ToLower(strings.TrimSpace(*answer.Shape))
	// Preserve the existing fixture's circle/circular equivalence without a
	// general synonym classifier or an answer-disclosing schema enum.
	if observedShape == "circular" {
		observedShape = "circle"
	}
	return strings.ToLower(strings.TrimSpace(*answer.Color)) == color && observedShape == shape
}

// AttachedImageHistory observes real user input, not the image-generation tool.
// One image-bearing turn and one text-only turn share one bound SDK thread.
func AttachedImageHistory(ctx context.Context, options Options) (Evidence, error) {
	options.attachedImageFixture = true
	server := &sdkProductObserver{ctx: ctx, changed: make(chan struct{}, 1), diagnostics: options.Diagnostics}
	handlers := sdkFixtureHandlers{requests: server.request, notifications: server.typedNotification, afterClose: server.afterClose,
		retention: codexsdk.NotificationRetentionBudget{MaxCount: 8192, MaxBytes: 8 << 20}}
	return withSDKFixture(ctx, options, nil, handlers, func(client *codexsdk.Client, cwd string, evidence *Evidence) error {
		server.client = client
		schema, err := protocolv2.OutputSchemaFromJSON([]byte(attachedImageSchemaJSON))
		if err != nil {
			return failedObservation("visual_schema_invalid", "live: visual reply schema unavailable")
		}
		fallback := false
		started, err := client.Threads().Start(ctx, protocolv2.ThreadStartParams{Model: protocolv2.Value(options.Model), ModelProvider: protocolv2.Value("grok"), CWD: protocolv2.Value(cwd), AllowProviderModelFallback: &fallback})
		if err != nil {
			return sdkFailure(err)
		}
		if started.Thread.ID == "" || started.Model != options.Model || started.ModelProvider != "grok" || started.Thread.ModelProvider != "grok" || started.CWD != cwd || started.Thread.CWD != cwd {
			return failedObservation("fixture_binding_failed", "live: fixture binding not established")
		}
		evidence.Threads, evidence.Bound = 1, true
		picture, err := attachedImageURL()
		if err != nil {
			return failedObservation("image_fixture_invalid", "live: input image unavailable")
		}
		pictureBytes, decodeErr := base64.StdEncoding.DecodeString(strings.TrimPrefix(picture, "data:image/png;base64,"))
		if decodeErr == nil {
			reference := options.Diagnostics.Artifact("attached-input.png", pictureBytes)
			options.Diagnostics.Record("attached_image_input", map[string]any{"artifact": reference, "sha256": editDigest(pictureBytes)})
		} else {
			options.Diagnostics.Record("attached_image_input", map[string]any{"capture": "decode_failed"})
		}
		pictureValue := protocolv2.JSONString(picture)
		inputs := [][]protocolv2.UserInput{
			{protocolv2.NewUserInputText(protocolv2.UserInputText{Text: attachedImageFirstPrompt}), protocolv2.NewUserInputImage(protocolv2.UserInputImage{URL: &pictureValue})},
			{protocolv2.NewUserInputText(protocolv2.UserInputText{Text: attachedImageNextPrompt})},
		}
		expected := [][2]string{{"red", "circle"}, {"blue", "square"}}
		var turns []string
		var terminalMu sync.Mutex
		var terminals []frame
		server.setNotification(func(message frame) error {
			if message.Method != "turn/completed" {
				return nil
			}
			terminalMu.Lock()
			defer terminalMu.Unlock()
			if len(terminals) == 128 {
				return failedObservation("terminal_budget_exceeded", "live: terminal evidence budget exceeded")
			}
			terminals = append(terminals, message)
			return nil
		})
		// Retain contradiction checking through joined callback cleanup, not only
		// until the first matching successful terminal.
		server.finalize = func() error {
			terminalMu.Lock()
			defer terminalMu.Unlock()
			for _, message := range terminals {
				for _, id := range turns {
					if _, err := productTerminal(message, started.Thread.ID, id); err != nil {
						return err
					}
				}
			}
			return nil
		}
		for index, input := range inputs {
			options.Diagnostics.Record("attached_image_phase", map[string]any{"turn": index + 1, "status": "submitted", "prompt": []string{attachedImageFirstPrompt, attachedImageNextPrompt}[index], "output_schema": schema})
			evidence.Completed = false
			thread, turn, err := observeProductInputTurn(server, started.Thread.ID, evidence, "", input, &schema)
			options.Diagnostics.Record("attached_image_phase", map[string]any{"turn": index + 1, "status": "returned", "completed": err == nil})
			diagnosticReply, diagnosticErr := finalProductReply(turn)
			options.Diagnostics.Record("attached_image_reply", map[string]any{"turn": index + 1, "reply": diagnosticReply, "aggregate_available": diagnosticErr == nil})
			evidence.Completed = false
			if err != nil {
				return err
			}
			for _, prior := range turns {
				if turn.ID == prior {
					return failedObservation("turn_identity_reused", "live: distinct continuation absent")
				}
			}
			turns = append(turns, turn.ID)
			for _, id := range turns {
				found := 0
				for _, retained := range thread.Turns {
					if retained.ID == id {
						found++
						if err := completedProductTurn(retained); err != nil {
							return err
						}
					}
				}
				if found != 1 {
					return failedObservation("turn_history_identity_invalid", "live: submitted image turn absent or duplicated")
				}
			}
			reply, err := finalProductReply(turn)
			if err != nil {
				return err
			}
			var answer attachedImageProperties
			parseErr := parseStructuredReply(reply, &answer, "color", "shape")
			accepted := parseErr == nil && answer.matches(expected[index][0], expected[index][1])
			options.Diagnostics.Record("attached_image_verdict", map[string]any{"turn": index + 1, "valid": parseErr == nil, "accepted": accepted, "properties": answer})
			if parseErr != nil {
				return failedObservation("visual_answer_invalid", "live: visual reply did not match the required JSON object")
			}
			if !accepted {
				return failedObservation("visual_answer_mismatch", "live: visual answer not established")
			}
			if index == 0 {
				evidence.FirstCompleted, evidence.FirstRecall, evidence.FirstReplyBytes = true, true, len(reply)
			} else {
				evidence.Recalled = true
			}
		}
		evidence.Completed, evidence.Stage = true, "attached_image_history_observed"
		return nil
	})
}
