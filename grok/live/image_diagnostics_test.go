package live

import "testing"

func TestImageJudgeRequiresBothJSONBooleans(t *testing.T) {
	for _, tc := range []struct {
		text          string
		first, second bool
	}{
		{`{"first_has_one_dog":true,"second_has_two_dogs":true}`, true, true},
		{" \n{\"second_has_two_dogs\":false,\"first_has_one_dog\":true}\t", true, false},
		{`{"first_has_one_dog":false,"second_has_two_dogs":true}`, false, true},
		{`{"first_has_one_dog":false,"second_has_two_dogs":false}`, false, false},
	} {
		first, second, err := imageJudgeAnswer(tc.text)
		if err != nil || first != tc.first || second != tc.second {
			t.Errorf("valid judgment mishandled: %q, %v, %v, %v", tc.text, first, second, err)
		}
	}
	for _, text := range []string{
		"", "null", "true", `"true"`, "1", `[true,true]`, `{}`, `{"first_has_one_dog":true}`,
		`{"first_has_one_dog":true,"second_has_two_dogs":null}`,
		`{"first_has_one_dog":"true","second_has_two_dogs":true}`,
		`{"first_has_one_dog":false,"first_has_one_dog":true,"second_has_two_dogs":true}`,
		`{"first_has_one_dog":true,"second_has_two_dogs":true,"extra":false}`,
		`{"first_has_one_dog":true,"second_has_two_dogs":true} {}`,
		"```json\n{\"first_has_one_dog\":true,\"second_has_two_dogs\":true}\n```",
	} {
		first, second, err := imageJudgeAnswer(text)
		if err == nil || first || second || DescribeFailure(err).Code != "image_judge_response_invalid" {
			t.Errorf("invalid judgment accepted: %q", text)
		}
	}
}
