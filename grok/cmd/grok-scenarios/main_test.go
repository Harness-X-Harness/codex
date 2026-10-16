package main

import (
	"bytes"
	"encoding/json"
	"io"
	"testing"

	"github.com/Harness-X-Harness/codex/grok/internal/scenarios"
)

func TestCommandUsesCatalogForEverySuite(t *testing.T) {
	for _, family := range []string{"live", "facts"} {
		for _, suite := range []string{"full", "smoke", "diagnostic"} {
			t.Run(family+"/"+suite, func(t *testing.T) {
				var output bytes.Buffer
				if code := run([]string{"--family", family, "--suite", suite}, &output); code != 0 {
					t.Fatalf("exit %d", code)
				}
				var names []string
				if json.Unmarshal(output.Bytes(), &names) != nil {
					t.Fatal("invalid matrix JSON")
				}
				want, err := scenarios.Select(family, suite, "")
				if err != nil || len(want) != len(names) {
					t.Fatal("CLI/catalog matrix drift")
				}
				for i, entry := range want {
					if names[i] != entry.Name {
						t.Fatal("CLI changed stable ordering")
					}
				}
			})
		}
	}
	var defaults, explicit bytes.Buffer
	if run(nil, &defaults) != 0 || run([]string{"--family", "live", "--suite", "full"}, &explicit) != 0 || defaults.String() != explicit.String() {
		t.Fatal("default must be formal Live full")
	}
}

func TestCommandSelectionRejectsUnknownWithoutEchoingInput(t *testing.T) {
	for _, args := range [][]string{{"--scenario", "PRIVATE_CANARY"}, {"--suite", "PRIVATE_CANARY"}, {"--family", "PRIVATE_CANARY"}, {"--PRIVATE_CANARY"}, {"PRIVATE_CANARY"}, {"--suite", "full", "--scenario", "reasoning-no-effort-pinned"}} {
		var output bytes.Buffer
		if run(args, &output) != 2 || output.Len() != 0 {
			t.Fatal("invalid selector succeeded or leaked input")
		}
	}
	var output bytes.Buffer
	if run([]string{"--suite", "diagnostic", "--scenario", "reasoning-no-effort-pinned"}, &output) != 0 || output.String() != "[\"reasoning-no-effort-pinned\"]\n" {
		t.Fatal("explicit diagnostic unavailable")
	}
	if run(nil, failedWriter{}) != 1 {
		t.Fatal("unwritten matrix succeeded")
	}
}

type failedWriter struct{}

func (failedWriter) Write([]byte) (int, error) { return 0, io.ErrClosedPipe }
