// Package testresults validates bounded go test -json evidence, never stdout
// summaries. A package pass alone is not evidence that a selected test ran.
package testresults

import (
	"bufio"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strings"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
)

const (
	maxInputBytes = 64 << 20
	maxEventBytes = 1 << 20
	maxEvents     = 500_000
	maxTests      = 100_000
)

// Test identifies one exact Go test owner, including its package.
type Test struct {
	Package string
	Name    string
}

// Summary counts observed test passes and packages with invoked tests.
type Summary struct {
	Tests    int `json:"tests"`
	Packages int `json:"packages"`
}

// IntegrationTests is the mandatory inventory of real-Codex fixture owners.
// Descendant passes cannot replace a missing owner, and a parent cannot replace
// its children. This is independent of the paid Live scenario registry.
func IntegrationTests() []Test {
	const pkg = "github.com/Harness-X-Harness/codex/grok/tests/integration"
	owners := []Test{
		{pkg, "TestBasicFixture"},
		{pkg, "TestBasicFixture/" + providerfixture.PrimaryModel},
		{pkg, "TestBasicFixture/" + providerfixture.PinnedModel},
		{pkg, "TestStructuredEditFixture"},
		{pkg, "TestStructuredEditFixture/continuation"},
		{pkg, "TestStructuredEditFixture/pinned"},
		{pkg, "TestStructuredEditFixture/declined"},
		{pkg, "TestStructuredEditFixture/replace_all"},
		{pkg, "TestReasoningHistoryFixture"},
		{pkg, "TestReasoningHistoryFixture/primary"},
		{pkg, "TestReasoningHistoryFixture/pinned"},
		{pkg, "TestHostedSearchFixture"},
		{pkg, "TestHostedSearchFixture/web"},
		{pkg, "TestHostedSearchFixture/web_allowed"},
		{pkg, "TestHostedSearchFixture/web_excluded"},
		{pkg, "TestHostedSearchFixture/x_retired"},
		{pkg, "TestHostedSearchFixture/x_window_retired"},
		{pkg, "TestHostedSearchFixture/unexpected_x_rejected"},
		{pkg, "TestImagePinnedFixture"},
		{pkg, "TestAttachedImagePinnedFixture"},
		{pkg, "TestShippedCatalog"},
		{pkg, "TestShippedThreadSource"},
		{pkg, "TestShippedChildFixture"},
		{pkg, "TestShippedChildFixture/fresh_child_result"},
		{pkg, "TestShippedChildFixture/parent_supplied_result_rejected"},
		{pkg, "TestFirstRequestShape"},
	}
	for _, model := range []string{providerfixture.PrimaryModel, providerfixture.PinnedModel} {
		for _, profile := range []string{"isolated-history", "shipped-history", "isolated-edit"} {
			owners = append(owners, Test{pkg, "TestFirstRequestShape/" + model + "/" + profile})
		}
	}
	return owners
}

// IntegrationOwnerTests selects complete owners from the same mandatory
// inventory. Package smoke scopes cannot replace a proof-bearing child with
// its nominal parent pass or maintain a second, incomplete child inventory.
func IntegrationOwnerTests(names []string) ([]Test, error) {
	wanted := make(map[string]bool, len(names))
	for _, name := range names {
		if _, duplicate := wanted[name]; duplicate || name == "" || strings.Contains(name, "/") {
			return nil, errors.New("testresults: invalid or duplicate integration owner")
		}
		wanted[name] = false
	}
	if len(wanted) == 0 {
		return nil, errors.New("testresults: empty integration owner selection")
	}
	var selected []Test
	for _, test := range IntegrationTests() {
		root, _, _ := strings.Cut(test.Name, "/")
		if _, include := wanted[root]; include {
			selected = append(selected, test)
			wanted[root] = true
		}
	}
	for _, found := range wanted {
		if !found {
			return nil, errors.New("testresults: unknown integration owner")
		}
	}
	return selected, nil
}

type event struct {
	Action, Package, Test, Output string
}

type packageState struct {
	terminal string
	noTests  bool
	tests    int
}

// Check rejects skips of invoked tests, failures, duplicate or incomplete
// lifecycles, empty selections and absent required owners. Go's package-only
// "[no test files]" skip is allowed only for a started package with no test run.
func Check(input io.Reader, required []Test) (Summary, error) {
	var summary Summary
	wanted := make(map[Test]bool, len(required))
	for _, test := range required {
		if test.Package == "" || test.Name == "" || wanted[test] {
			return summary, errors.New("testresults: invalid or duplicate required test")
		}
		wanted[test] = true
	}
	packages := map[string]*packageState{}
	tests := map[Test]string{}
	limited := &io.LimitedReader{R: input, N: maxInputBytes + 1}
	scanner := bufio.NewScanner(limited)
	scanner.Buffer(make([]byte, 4096), maxEventBytes)
	events := 0
	for scanner.Scan() {
		events++
		if events > maxEvents || limited.N <= 0 {
			return summary, errors.New("testresults: event budget exceeded")
		}
		var e event
		if json.Unmarshal(scanner.Bytes(), &e) != nil || e.Package == "" || len(e.Package) > 8192 || len(e.Test) > 8192 {
			return summary, errors.New("testresults: invalid Go test event")
		}
		pkg := packages[e.Package]
		if e.Action == "start" {
			if e.Test != "" || pkg != nil {
				return summary, errors.New("testresults: duplicate or invalid package start")
			}
			packages[e.Package] = &packageState{}
			continue
		}
		if pkg == nil || pkg.terminal != "" {
			return summary, errors.New("testresults: event outside package lifecycle")
		}
		key := Test{e.Package, e.Test}
		state := tests[key]
		switch e.Action {
		case "run":
			if e.Test == "" || state != "" || len(tests) >= maxTests {
				return summary, errors.New("testresults: duplicate or invalid test run")
			}
			tests[key], pkg.tests = "run", pkg.tests+1
		case "pause":
			if e.Test == "" || state != "run" {
				return summary, errors.New("testresults: invalid test pause")
			}
			tests[key] = "pause"
		case "cont":
			if e.Test == "" || state != "pause" {
				return summary, errors.New("testresults: invalid test continuation")
			}
			tests[key] = "run"
		case "pass":
			if e.Test == "" {
				pkg.terminal = "pass"
			} else {
				if state != "run" {
					return summary, errors.New("testresults: duplicate, incomplete or unstarted test terminal")
				}
				tests[key] = "pass"
				summary.Tests++
			}
		case "skip":
			if e.Test != "" || pkg.tests != 0 || !pkg.noTests {
				return summary, errors.New("testresults: skipped required execution")
			}
			pkg.terminal = "skip"
		case "fail", "build-fail":
			return summary, errors.New("testresults: failed execution")
		case "output":
			if e.Test != "" && state == "" {
				return summary, errors.New("testresults: output without test run")
			}
			if e.Test == "" && strings.Contains(e.Output, "[no test files]") {
				pkg.noTests = true
			}
		default:
			return summary, errors.New("testresults: unknown event action")
		}
	}
	if scanner.Err() != nil || limited.N <= 0 {
		return summary, errors.New("testresults: unreadable or oversized Go test results")
	}
	if summary.Tests == 0 {
		return summary, errors.New("testresults: no tests passed")
	}
	for name, pkg := range packages {
		if pkg.terminal == "" || (pkg.tests > 0 && pkg.terminal != "pass") {
			return summary, fmt.Errorf("testresults: incomplete package %s", name)
		}
		if pkg.tests > 0 {
			summary.Packages++
		}
	}
	for test, state := range tests {
		if state != "pass" {
			return summary, fmt.Errorf("testresults: incomplete test %s:%s", test.Package, test.Name)
		}
	}
	for _, test := range required {
		if tests[test] != "pass" {
			return summary, fmt.Errorf("testresults: required test absent: %s:%s", test.Package, test.Name)
		}
	}
	return summary, nil
}
