package live

import (
	"strings"
	"testing"
)

func TestSearchStructuredResultContracts(t *testing.T) {
	for _, answer := range []string{
		`{"articles":[{"headline":"Any source headline","url":"https://news.example.test/a"}],"summary":null}`,
		`{"articles":[{"headline":"Headline","url":"http://example.org/a"}],"summary":"Context"}`,
	} {
		if err := validateSearchAnswer(answer, false); err != nil {
			t.Errorf("valid articles rejected: %v", err)
		}
	}
	if err := validateSearchAnswer(`{"summary":"Prior result summary"}`, true); err != nil {
		t.Fatal(err)
	}
	for _, answer := range []string{
		`null`, `[]`, `true`,
		`{"articles":[{"Headline":"h","url":"https://example.test"}],"summary":null}`,
		`{"articles":[null],"summary":null}`, `{"articles":null,"summary":null}`, `{"articles":[],"summary":null}`,
		`{"articles":[{"headline":"h","url":"https://example.test","extra":1}],"summary":null}`,
		`{"articles":[{"headline":"h","headline":"other","url":"https://example.test"}],"summary":null}`,
		`{"articles":[{"headline":"h","url":null}],"summary":null}`,
		`{"articles":[{"headline":1,"url":"https://example.test"}],"summary":null}`,
		`{"articles":[{"headline":" ","url":"https://example.test"}],"summary":null}`,
		`{"articles":[{"headline":"h","url":"/relative"}],"summary":null}`,
		`{"articles":[{"headline":"h","url":"https://user:pass@example.test"}],"summary":null}`,
		`{"articles":[{"headline":"h","url":"https://example.test"}],"summary":1}`,
		`{"articles":[{"headline":"h","url":"https://example.test"}],"summary":null} true`,
		`{"articles":[` + strings.TrimSuffix(strings.Repeat(`{"headline":"h","url":"https://example.test"},`, 9), ",") + `],"summary":null}`,
	} {
		if err := validateSearchAnswer(answer, false); err == nil {
			t.Errorf("invalid articles accepted: %s", answer)
		}
	}
	for _, answer := range []string{`{}`, `{"summary":null}`, `{"summary":" "}`, `{"summary":1}`, `{"summary":"s","articles":[]}`, `{"summary":"s","summary":"s"}`, `"summary"`} {
		if err := validateSearchAnswer(answer, true); err == nil {
			t.Errorf("invalid continuation accepted: %s", answer)
		}
	}
}

func TestSearchSchemaLengthsCountUnicodeCodepoints(t *testing.T) {
	headline := func(text string) string {
		return `{"articles":[{"headline":"` + text + `","url":"https://news.example.test/a"}],"summary":null}`
	}
	for _, text := range []string{strings.Repeat("汉", 200), strings.Repeat("汉", 512)} {
		if err := validateSearchAnswer(headline(text), false); err != nil {
			t.Errorf("schema-valid Unicode headline rejected: %v", err)
		}
	}
	if err := validateSearchAnswer(headline(strings.Repeat("汉", 513)), false); err == nil {
		t.Error("headline over schema codepoint limit accepted")
	}
	// Field constraints count codepoints; the independent total reply budget is bytes.
	if err := validateSearchAnswer(`{"summary":"`+strings.Repeat("汉", 1000)+`"}`, true); err != nil {
		t.Errorf("schema-valid Unicode summary rejected: %v", err)
	}
	if err := validateSearchAnswer(`{"summary":"`+strings.Repeat("a", 2049)+`"}`, true); err == nil {
		t.Error("summary over codepoint limit accepted")
	}
	if err := validateSearchAnswer(`{"summary":"`+strings.Repeat("汉", 1500)+`"}`, true); err == nil {
		t.Error("global byte budget bypassed")
	}
}
