package live

import (
	"context"
	"encoding/json"
	"strings"
	"unicode"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// ImageEvidence reports public image results and independent visual answers.
// Digests identify observed bytes; they do not prove their visual meaning.
type ImageEvidence struct {
	Evidence
	TurnCompleted, GenerationRecognized, EditRecognized bool
	Images, Artifacts                                   int
	FirstImageSHA256, EditedImageSHA256                 string
}

// ImageGenerationEdit generates a dog, edits it in the same conversation, and
// asks separate fresh conversations to describe each actual output image.
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

// A deliberately small English fixture oracle. Wording and sentence structure
// are free; the expected visual facts must occur as words, not prompt echoes or
// substrings. Explicit negative/uncertain answers do not establish recognition.
func imageRecognized(reply string, edited bool) bool {
	words := strings.FieldsFunc(strings.ToLower(reply), func(r rune) bool { return !unicode.IsLetter(r) })
	seen := map[string]bool{}
	required := map[string]bool{"dog": true}
	if edited {
		required["red"], required["hat"] = true, true
	}
	for index, word := range words {
		seen[word] = true
		switch word {
		case "cannot", "cant", "unsure", "maybe", "possibly":
			return false
		case "no", "not", "without":
			// Reject absent requested facts, not unrelated negatives such as
			// "a dog with no accessories" or "red hat, no other clothing".
			end := index + 5
			if end > len(words) {
				end = len(words)
			}
			for _, next := range words[index+1 : end] {
				if next == "but" {
					break
				}
				if required[next] {
					return false
				}
			}
		}
	}
	return seen["dog"] && (!edited || seen["red"] && seen["hat"])
}

func nullJSON(value json.RawMessage) bool {
	return len(value) == 0 || strings.TrimSpace(string(value)) == "null"
}

func imageInput(image observedImage) protocolv2.UserInput {
	url := protocolv2.JSONString("data:image/png;base64," + image.payload)
	return protocolv2.NewUserInputImage(protocolv2.UserInputImage{URL: &url})
}
