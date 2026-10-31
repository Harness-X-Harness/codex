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

func TestSearchCompleteResultsDoNotDependOnIncidentalLengthOrArticleCount(t *testing.T) {
	article := `{"headline":"h","url":"https://example.test"}`
	manyArticles := strings.TrimSuffix(strings.Repeat(article+",", 70), ",")
	for _, answer := range []string{
		`{"articles":[` + manyArticles + `],"summary":null}`,
		`{"articles":[{"headline":"` + strings.Repeat("汉", 2000) + `","url":"https://news.example.test/` + strings.Repeat("a", 4096) + `"}],"summary":"` + strings.Repeat("汉", 3000) + `"}`,
	} {
		if err := validateSearchAnswer(answer, false); err != nil {
			t.Errorf("complete valid search result rejected: %v", err)
		}
	}
	summary := `{"summary":"` + strings.Repeat("汉", 6000) + `"}`
	if err := validateSearchAnswer(summary, true); err != nil {
		t.Errorf("complete valid summary rejected: %v", err)
	}
	for _, answer := range []string{summary + " trailing", summary + summary, strings.TrimSuffix(summary, "}")} {
		if err := validateSearchAnswer(answer, true); err == nil {
			t.Fatal("incomplete or trailing content accepted after a valid long prefix")
		}
	}
	for _, invalidLast := range []string{`null`, `{"headline":"h","url":"javascript:alert(1)"}`, `{"headline":"h","url":"https://example.test","url":"https://other.test"}`} {
		if err := validateSearchAnswer(`{"articles":[`+manyArticles+","+invalidLast+`],"summary":null}`, false); err == nil {
			t.Fatal("invalid article beyond the old count limits was not checked")
		}
	}
}
