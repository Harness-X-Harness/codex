package live

import "testing"

func TestImageJudgeRequiresOneJSONBoolean(t *testing.T) {
	for _, text := range []string{"true", " \ntrue\t"} {
		accepted, err := imageJudgeAnswer(text)
		if err != nil || !accepted {
			t.Errorf("true rejected: %q, %v", text, err)
		}
	}
	for _, text := range []string{"false", " false \n"} {
		accepted, err := imageJudgeAnswer(text)
		if err != nil || accepted {
			t.Errorf("false mishandled: %q, %v", text, err)
		}
	}
	for _, text := range []string{"", "null", `"true"`, "1", `{"answer":true}`, "[true]", "True", "TRUE", "true false", "true because a dog is visible", "```json\ntrue\n```", "tru"} {
		accepted, err := imageJudgeAnswer(text)
		if err == nil || accepted {
			t.Errorf("invalid boolean response accepted: %q", text)
		}
	}
}

func TestAttachedImageScopesUnrelatedQualifiers(t *testing.T) {
	for _, text := range []string{
		"A red circle, possibly made of paper.",
		"An object of unknown material shaped as a red circle.",
		"No border. A red circle is on the left.",
		"The left object is a red circle. I cannot identify its material.",
	} {
		if !attachedImageAnswer(text, "red", "circle", "left") {
			t.Errorf("valid shape rejected: %q", text)
		}
	}
	for _, text := range []string{
		"Possibly a red circle.", "I cannot tell whether it is a red circle.",
		"Not a red circle.", "A blue square.", "A red circle on the right.",
	} {
		if attachedImageAnswer(text, "red", "circle", "left") {
			t.Errorf("wrong shape accepted: %q", text)
		}
	}
}
