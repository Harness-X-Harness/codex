package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
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
