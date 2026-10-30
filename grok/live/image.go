package live

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// ImageEvidence reports public image results and a producer-isolated visual judgment.
// Digests identify observed bytes; they do not prove their visual meaning.
type ImageEvidence struct {
	Evidence
	TurnCompleted, GenerationRecognized, EditRecognized bool
	Images, Artifacts                                   int
	FirstImageSHA256, EditedImageSHA256                 string
}

// ImageGenerationEdit generates a dog, edits it in the same conversation, and
// asks one fresh conversation to judge both actual output images using
// schema-constrained booleans, not lexical matching of image descriptions.
func ImageGenerationEdit(ctx context.Context, options Options) (ImageEvidence, error) {
	options.imageFixture = true
	return sdkImageGenerationEdit(ctx, options)
}

type observedImage struct{ payload, path, digest string }

// The last completed public image is the user-visible result of this turn.
// Intermediate errors, tool choices and prepared history representations are
// not the outcome. No raw protocol extension is needed to credit that result.
func completedImage(turn productTurn, root string) (observedImage, error) {
	var result observedImage
	for _, item := range turn.Items {
		image, ok := item.AsImageGeneration()
		if !ok || image.Status != "completed" {
			continue
		}
		if image.Failure != nil && image.Failure.Value != nil {
			continue
		}
		result = observedImage{payload: image.Result, path: sdkString(image.SavedPath)}
	}
	if result.payload == "" {
		return result, failedObservation("image_result_missing", "live: completed image result unavailable")
	}
	data, err := imagePayload(result.payload)
	if err != nil {
		return result, err
	}
	result.digest = editDigest(data)
	if _, err := inspectImage(root, result.path, result.digest); err != nil {
		return result, err
	}
	return result, nil
}

// The judge sees the ordered image pair once. Both visual propositions remain
// required; this parser checks their response contract, not visual correctness.
type imageJudgment struct {
	FirstHasOneDog   *bool `json:"first_has_one_dog"`
	SecondHasTwoDogs *bool `json:"second_has_two_dogs"`
}

func imageJudgeAnswer(reply string) (bool, bool, error) {
	var value imageJudgment
	if err := parseStructuredReply(reply, &value, "first_has_one_dog", "second_has_two_dogs"); err != nil || value.FirstHasOneDog == nil || value.SecondHasTwoDogs == nil {
		return false, false, failedObservation("image_judge_response_invalid", "live: image judge did not return both boolean judgments")
	}
	return *value.FirstHasOneDog, *value.SecondHasTwoDogs, nil
}

// Diagnostics retain every public image attempt, not just the final image
// credited by the oracle. Capture never changes acceptance or reads saved paths.
func recordImageTurn(diagnostics *Diagnostics, phase string, turn productTurn) {
	for index, item := range turn.Items {
		image, ok := item.AsImageGeneration()
		if !ok {
			continue
		}
		fields := map[string]any{"phase": phase, "item": index, "status": image.Status,
			"failed": image.Failure != nil && image.Failure.Value != nil}
		if image.Result != "" {
			data, err := imagePayload(image.Result)
			if err != nil {
				fields["capture"] = "invalid_png"
			} else {
				fields["artifact"] = diagnostics.Artifact(fmt.Sprintf("%s-%d.png", phase, index), data)
				fields["sha256"] = editDigest(data)
			}
		} else {
			fields["capture"] = "no_image_payload"
		}
		diagnostics.Record("image_attempt", fields)
	}
}

func nullJSON(value json.RawMessage) bool {
	return len(value) == 0 || strings.TrimSpace(string(value)) == "null"
}

func imageInput(image observedImage) protocolv2.UserInput {
	url := protocolv2.JSONString("data:image/png;base64," + image.payload)
	return protocolv2.NewUserInputImage(protocolv2.UserInputImage{URL: &url})
}
