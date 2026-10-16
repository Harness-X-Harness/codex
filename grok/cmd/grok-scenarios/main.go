// Command grok-scenarios emits the authoritative, credential-free CI matrix.
package main

import (
	"encoding/json"
	"flag"
	"io"
	"os"

	"github.com/Harness-X-Harness/codex/grok/internal/scenarios"
)

func main() { os.Exit(run(os.Args[1:], os.Stdout)) }

func run(args []string, output io.Writer) int {
	flags := flag.NewFlagSet("grok-scenarios", flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	family := flags.String("family", "live", "live or facts")
	suite := flags.String("suite", "full", "smoke, full or diagnostic")
	name := flags.String("scenario", "", "optional exact scenario")
	if err := flags.Parse(args); err != nil {
		if err == flag.ErrHelp {
			if _, err := io.WriteString(output, "Usage: grok-scenarios [--family live|facts] [--suite smoke|full|diagnostic] [--scenario NAME]\n"); err != nil {
				return 1
			}
			return 0
		}
		return 2
	}
	if flags.NArg() != 0 {
		return 2
	}
	selected, err := scenarios.Select(*family, *suite, *name)
	if err != nil {
		return 2
	}
	names := make([]string, 0, len(selected))
	for _, scenario := range selected {
		names = append(names, scenario.Name)
	}
	if json.NewEncoder(output).Encode(names) != nil {
		return 1
	}
	return 0
}
