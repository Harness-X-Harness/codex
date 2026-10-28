package live

import (
	"strings"
	"testing"
)

func TestStructuredReplyOneCompleteTypedObject(t *testing.T) {
	type result struct {
		Token *string `json:"token"`
	}
	for _, raw := range []string{`{"token":"actual"}`, ` {"token":null} `} {
		var got result
		if err := parseStructuredReply(raw, &got, "token"); err != nil {
			t.Errorf("valid shape rejected %s: %v", raw, err)
		}
	}
	for _, raw := range []string{"", `null`, `true`, `[]`, `{}`, `{"Token":"wrong case"}`, `{"token":42}`, `{"token":true}`, `{"token":{}}`, `{"token":"a","extra":true}`, `{"token":"a","token":"b"}`, `{"token":"a"} {"token":"b"}`, "```json\n{\"token\":\"a\"}\n```", `{"token":"a"} trailing`, `{"token":"` + strings.Repeat("x", 4096) + `"}`} {
		var got result
		if parseStructuredReply(raw, &got, "token") == nil {
			t.Errorf("invalid result accepted %s", raw)
		}
	}
}

func TestStructuredReplyRejectsNestedDuplicatesAndUnknownFields(t *testing.T) {
	type article struct {
		Headline *string `json:"headline"`
		URL      *string `json:"url"`
	}
	type result struct {
		Articles []article `json:"articles"`
	}
	for _, raw := range []string{`{"articles":[{"headline":"a","url":"https://example.test","url":"https://other.test"}]}`, `{"articles":[{"headline":"a","url":"https://example.test","extra":true}]}`} {
		var got result
		if parseStructuredReply(raw, &got, "articles") == nil {
			t.Errorf("invalid nested result accepted %s", raw)
		}
	}
}
