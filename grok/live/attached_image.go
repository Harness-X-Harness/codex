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
	"unicode"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

const attachedImageFirstPrompt = "Look at the attached picture. Describe the color and shape of the object on the left in English."
const attachedImageNextPrompt = "From the picture attached earlier in this same conversation, describe the color and shape of the object on the right in English."

// attachedImageURL carries the answers only in pixels, not a filename or prompt.
// The second fact is deliberately absent from the first accepted reply.
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

// This fixture asks for two simple visual properties, not a particular sentence.
// Ordinary descriptive words and sentence structure do not decide acceptance.
// The finite checks reject explicit denial, uncertainty and conflicting fixture
// properties; they are not a general natural-language judge.
func attachedImageWords(text string) []string {
	return strings.FieldsFunc(strings.ToLower(text), func(r rune) bool { return unicode.IsSpace(r) || unicode.IsPunct(r) })
}

func attachedImageAnswer(text, expectedColor, expectedShape, side string) bool {
	if strings.Contains(text, "?") {
		return false
	}
	words := attachedImageWords(strings.ReplaceAll(strings.ReplaceAll(strings.ToLower(text), "can’t", "cannot"), "can't", "cannot"))
	for index, word := range words {
		if word == "circular" {
			words[index] = "circle"
		}
	}
	color, shape := false, false
	for index, word := range words {
		switch word {
		case expectedColor:
			color = true
		case expectedShape:
			shape = true
		case "cannot", "maybe", "perhaps", "might", "unsure", "unknown":
			return false
		case "no", "not", "without":
			// Unrelated negatives such as "a red circle with no border" are
			// ordinary description, not a denial of the required visual facts.
			end := index + 5
			if end > len(words) {
				end = len(words)
			}
			for _, next := range words[index+1 : end] {
				if next == "but" {
					break
				}
				if next == expectedColor || next == expectedShape {
					return false
				}
			}
		case "red", "blue", "circle", "square":
			// These are the two actual objects in the fixture. Naming both is
			// ambiguous rather than proof of the requested object's properties.
			return false
		case "left", "right":
			if word != side {
				return false
			}
		}
	}
	return color && shape
}

// AttachedImageHistory observes real user input, not the image-generation tool.
// One image-bearing turn and one text-only turn share one bound SDK thread.
func AttachedImageHistory(ctx context.Context, options Options) (Evidence, error) {
	options.attachedImageFixture = true
	server := &sdkProductObserver{ctx: ctx, changed: make(chan struct{}, 1)}
	handlers := sdkFixtureHandlers{requests: server.request, notifications: server.typedNotification, afterClose: server.afterClose,
		retention: codexsdk.NotificationRetentionBudget{MaxCount: 8192, MaxBytes: 8 << 20}}
	return withSDKFixture(ctx, options, nil, handlers, func(client *codexsdk.Client, cwd string, evidence *Evidence) error {
		server.client = client
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
		pictureValue := protocolv2.JSONString(picture)
		inputs := [][]protocolv2.UserInput{
			{protocolv2.NewUserInputText(protocolv2.UserInputText{Text: attachedImageFirstPrompt}), protocolv2.NewUserInputImage(protocolv2.UserInputImage{URL: &pictureValue})},
			{protocolv2.NewUserInputText(protocolv2.UserInputText{Text: attachedImageNextPrompt})},
		}
		expected := [][3]string{{"red", "circle", "left"}, {"blue", "square", "right"}}
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
			evidence.Completed = false
			thread, turn, err := observeProductInputTurn(server, started.Thread.ID, evidence, "", input)
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
			if !attachedImageFirstReplyIsolated(thread, turns[0]) {
				return failedObservation("visual_answer_leaked", "live: first reply disclosed more than the requested visual fact")
			}
			reply, err := finalProductReply(turn)
			if err != nil {
				return err
			}
			if !attachedImageAnswer(reply, expected[index][0], expected[index][1], expected[index][2]) {
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

// Public commentary is also available to the continuation; it must not reveal
// the second visual fact even when the eligible final answer is correct.
func attachedImageFirstReplyIsolated(thread productThread, firstID string) bool {
	for _, turn := range thread.Turns {
		if turn.ID != firstID {
			continue
		}
		for _, item := range turn.Items {
			if reply, ok := item.AsAgentMessage(); ok {
				for _, word := range attachedImageWords(reply.Text) {
					if word == "blue" || word == "square" {
						return false
					}
				}
			}
		}
	}
	return true
}
