package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/Harness-X-Harness/codex/grok/internal/testresults"
)

const validResults = `{"Action":"start","Package":"p"}
{"Action":"run","Package":"p","Test":"TestOne"}
{"Action":"pass","Package":"p","Test":"TestOne"}
{"Action":"pass","Package":"p"}
`

func TestResultCommand(t *testing.T) {
	for _, tc := range []struct {
		name, input string
		args        []string
		code        int
	}{
		{"stdin", validResults, nil, 0},
		{"required", validResults, []string{"--require", "p:TestOne"}, 0},
		{"integration_incomplete", validResults, []string{"--integration"}, 1},
		{"integration_owner_incomplete", validResults, []string{"--integration-owner", "TestBasicFixture"}, 1},
		{"integration_owner_unknown", validResults, []string{"--integration-owner", "TestUnknown"}, 2},
		{"integration_owner_duplicate", validResults, []string{"--integration-owner", "TestBasicFixture", "--integration-owner", "TestBasicFixture"}, 2},
		{"integration_owner_conflict", validResults, []string{"--integration", "--integration-owner", "TestBasicFixture"}, 2},
		{"empty", "", nil, 1},
		{"missing_owner", validResults, []string{"--require", "p:TestMissing"}, 1},
		{"duplicate_owner", validResults, []string{"--require", "p:TestOne", "--require", "p:TestOne"}, 1},
		{"invalid_flag", validResults, []string{"--require", "invalid"}, 2},
	} {
		t.Run(tc.name, func(t *testing.T) {
			var out, diagnostic bytes.Buffer
			if code := run(tc.args, strings.NewReader(tc.input), &out, &diagnostic); code != tc.code {
				t.Fatalf("code=%d output=%s diagnostic=%s", code, out.String(), diagnostic.String())
			}
			if tc.code != 2 {
				var summary struct {
					Tests, Packages int
					Passed          bool
				}
				if json.Unmarshal(out.Bytes(), &summary) != nil || summary.Passed != (tc.code == 0) {
					t.Fatalf("summary missing or wrong: %s", out.String())
				}
			}
		})
	}
}

func TestResultCommandIntegrationOwnerProofs(t *testing.T) {
	selected, err := testresults.IntegrationOwnerTests([]string{"TestBasicFixture"})
	if err != nil {
		t.Fatal(err)
	}
	for _, parentOnly := range []bool{false, true} {
		var input, out, diagnostic bytes.Buffer
		enc := json.NewEncoder(&input)
		pkg := selected[0].Package
		if err := enc.Encode(map[string]string{"Action": "start", "Package": pkg}); err != nil {
			t.Fatal(err)
		}
		for _, test := range selected {
			if parentOnly && strings.Contains(test.Name, "/") {
				continue
			}
			for _, action := range []string{"run", "pass"} {
				if err := enc.Encode(map[string]string{"Action": action, "Package": pkg, "Test": test.Name}); err != nil {
					t.Fatal(err)
				}
			}
		}
		if err := enc.Encode(map[string]string{"Action": "pass", "Package": pkg}); err != nil {
			t.Fatal(err)
		}
		code := run([]string{"--integration-owner", "TestBasicFixture"}, &input, &out, &diagnostic)
		if (code == 0) == parentOnly {
			t.Fatalf("parentOnly=%v code=%d diagnostic=%s", parentOnly, code, diagnostic.String())
		}
	}
}

func TestResultCommandInputFile(t *testing.T) {
	path := filepath.Join(t.TempDir(), "test.jsonl")
	if err := os.WriteFile(path, []byte(validResults), 0600); err != nil {
		t.Fatal(err)
	}
	for _, args := range [][]string{{path}, {"--input", path}} {
		var out, diagnostic bytes.Buffer
		if code := run(args, strings.NewReader(""), &out, &diagnostic); code != 0 {
			t.Fatalf("code=%d diagnostic=%s", code, diagnostic.String())
		}
	}
}
