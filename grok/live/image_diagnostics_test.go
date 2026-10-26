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
