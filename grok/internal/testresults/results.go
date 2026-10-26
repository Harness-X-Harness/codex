// Package testresults validates bounded go test -json evidence, never stdout
// summaries. A package pass alone is not evidence that a selected test ran.
package testresults

import (
	"bufio"
	"encoding/json"
	"errors"
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

// Summary preserves observed lifecycle counts, including on failure. Tests and
// Packages retain their legacy meaning: passed tests and passed packages that
// invoked tests. InputComplete means the entire input was parsed, not that all
// observed lifecycles completed successfully. Invalid input counts only its
// trusted prefix; no raw test names, output or reader errors are exported.
type Summary struct {
	Tests              int    `json:"tests"`
	Packages           int    `json:"packages"`
	TestsStarted       int    `json:"tests_started"`
	TestsPassed        int    `json:"tests_passed"`
	TestsFailed        int    `json:"tests_failed"`
	TestsSkipped       int    `json:"tests_skipped"`
	TestsIncomplete    int    `json:"tests_incomplete"`
	PackagesStarted    int    `json:"packages_started"`
	PackagesPassed     int    `json:"packages_passed"`
	PackagesFailed     int    `json:"packages_failed"`
	PackagesSkipped    int    `json:"packages_skipped"`
	PackagesIncomplete int    `json:"packages_incomplete"`
	InputComplete      bool   `json:"input_complete"`
	FailureReason      string `json:"failure_reason,omitempty"`
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
func Check(input io.Reader, required []Test) (summary Summary, err error) {
	packages := map[string]*packageState{}
	tests := map[Test]string{}
	var failureReason string
	// Finalize counts on every exit, including malformed or truncated streams.
	defer func() {
		summary.FailureReason = failureReason
		summary.TestsStarted = len(tests)
		for _, state := range tests {
			switch state {
			case "pass":
				summary.TestsPassed++
			case "fail":
				summary.TestsFailed++
			case "skip":
				summary.TestsSkipped++
			default:
				summary.TestsIncomplete++
			}
		}
		summary.PackagesStarted = len(packages)
		for _, pkg := range packages {
			switch pkg.terminal {
			case "pass":
				summary.PackagesPassed++
				if pkg.tests > 0 {
					summary.Packages++
				}
			case "fail":
				summary.PackagesFailed++
			case "skip":
				summary.PackagesSkipped++
			default:
				summary.PackagesIncomplete++
			}
		}
		summary.Tests = summary.TestsPassed
	}()
	reject := func(reason string) error {
		failureReason = reason
		return errors.New("testresults: " + reason)
	}
	wanted := make(map[Test]bool, len(required))
	for _, test := range required {
		if test.Package == "" || test.Name == "" || wanted[test] {
			return summary, reject("invalid_required_test")
		}
		wanted[test] = true
	}
	limited := &io.LimitedReader{R: input, N: maxInputBytes + 1}
	scanner := bufio.NewScanner(limited)
	scanner.Buffer(make([]byte, 4096), maxEventBytes)
	events := 0
	failed, skipped := false, false
	for scanner.Scan() {
		events++
		if events > maxEvents || limited.N <= 0 {
			return summary, reject("event_budget_exceeded")
		}
		var e event
		if json.Unmarshal(scanner.Bytes(), &e) != nil || e.Package == "" || len(e.Package) > 8192 || len(e.Test) > 8192 {
			return summary, reject("invalid_event")
		}
		pkg := packages[e.Package]
		if e.Action == "start" {
			if e.Test != "" || pkg != nil {
				return summary, reject("invalid_package_start")
			}
			packages[e.Package] = &packageState{}
			continue
		}
		if pkg == nil || pkg.terminal != "" {
			return summary, reject("outside_package_lifecycle")
		}
		key := Test{e.Package, e.Test}
		state := tests[key]
		switch e.Action {
		case "run":
			if e.Test == "" || state != "" || len(tests) >= maxTests {
				return summary, reject("invalid_test_run")
			}
			tests[key], pkg.tests = "run", pkg.tests+1
		case "pause":
			if e.Test == "" || state != "run" {
				return summary, reject("invalid_test_pause")
			}
			tests[key] = "pause"
		case "cont":
			if e.Test == "" || state != "pause" {
				return summary, reject("invalid_test_continuation")
			}
			tests[key] = "run"
		case "pass", "fail", "skip", "build-fail":
			terminal := e.Action
			if terminal == "build-fail" {
				if e.Test != "" {
					return summary, reject("invalid_test_terminal")
				}
				terminal = "fail"
			}
			if e.Test == "" {
				pkg.terminal = terminal
				if terminal == "skip" && (pkg.tests != 0 || !pkg.noTests) {
					skipped = true
				}
			} else {
				if state != "run" {
					return summary, reject("invalid_test_terminal")
				}
				tests[key] = terminal
				if terminal == "skip" {
					skipped = true
				}
			}
			if terminal == "fail" {
				failed = true
			}
		case "output":
			if e.Test != "" && state == "" {
				return summary, reject("output_without_test_run")
			}
			if e.Test == "" && strings.Contains(e.Output, "[no test files]") {
				pkg.noTests = true
			}
		default:
			return summary, reject("unknown_event_action")
		}
	}
	if scanner.Err() != nil || limited.N <= 0 {
		return summary, reject("unreadable_or_oversized_results")
	}
	summary.InputComplete = true
	// Failures and skips are observations, not parse errors. Consume all valid
	// events before rejecting the aggregate so later outcomes are not discarded.
	if failed {
		return summary, reject("failed_execution")
	}
	if skipped {
		return summary, reject("skipped_execution")
	}
	passed := 0
	for _, state := range tests {
		if state != "pass" {
			return summary, reject("incomplete_test")
		}
		passed++
	}
	for _, pkg := range packages {
		if pkg.terminal == "" {
			return summary, reject("incomplete_package")
		}
	}
	if passed == 0 {
		return summary, reject("no_tests_passed")
	}
	for _, test := range required {
		if tests[test] != "pass" {
			return summary, reject("required_test_absent")
		}
	}
	return summary, nil
}
