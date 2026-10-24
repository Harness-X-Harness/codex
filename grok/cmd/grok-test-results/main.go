// grok-test-results checks bounded go test -json lifecycle evidence.
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/Harness-X-Harness/codex/grok/internal/testresults"
)

type result struct {
	testresults.Summary
	Passed bool `json:"passed"`
}

type requiredTests []testresults.Test

func (tests *requiredTests) String() string { return "package:TestName" }
func (tests *requiredTests) Set(value string) error {
	pkg, name, ok := strings.Cut(value, ":")
	if !ok || pkg == "" || name == "" {
		return fmt.Errorf("require must be package:TestName")
	}
	*tests = append(*tests, testresults.Test{Package: pkg, Name: name})
	return nil
}

func run(args []string, stdin io.Reader, stdout, stderr io.Writer) int {
	flags := flag.NewFlagSet("grok-test-results", flag.ContinueOnError)
	flags.SetOutput(stderr)
	inputPath := flags.String("input", "", "Go JSON results file (default stdin)")
	integration := flags.Bool("integration", false, "require all integration owners and mandatory scenarios")
	var integrationOwners []string
	flags.Func("integration-owner", "require an integration owner and all mandatory scenarios (repeatable)", func(name string) error {
		integrationOwners = append(integrationOwners, name)
		return nil
	})
	var required requiredTests
	flags.Var(&required, "require", "require exact package:TestName (repeatable)")
	if flags.Parse(args) != nil || flags.NArg() > 1 || (*inputPath != "" && flags.NArg() != 0) {
		return 2
	}
	if *integration && len(integrationOwners) != 0 {
		fmt.Fprintln(stderr, "choose all integration owners or explicit owners, not both")
		return 2
	}
	if *integration {
		required = append(required, testresults.IntegrationTests()...)
	} else if len(integrationOwners) != 0 {
		selected, err := testresults.IntegrationOwnerTests(integrationOwners)
		if err != nil {
			fmt.Fprintln(stderr, err)
			return 2
		}
		required = append(required, selected...)
	}
	input := stdin
	if flags.NArg() == 1 {
		*inputPath = flags.Arg(0)
	}
	if *inputPath != "" {
		file, err := os.Open(*inputPath)
		if err != nil {
			_ = json.NewEncoder(stdout).Encode(result{})
			fmt.Fprintln(stderr, "testresults: results file unavailable")
			return 1
		}
		defer file.Close()
		input = file
	}
	summary, err := testresults.Check(input, required)
	report := result{summary, err == nil}
	if json.NewEncoder(stdout).Encode(report) != nil {
		return 1
	}
	if err != nil {
		fmt.Fprintln(stderr, err)
		return 1
	}
	return 0
}

func main() { os.Exit(run(os.Args[1:], os.Stdin, os.Stdout, os.Stderr)) }
