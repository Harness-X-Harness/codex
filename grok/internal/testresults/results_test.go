package testresults

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"strings"
	"testing"
)

func encode(events ...event) string {
	var out bytes.Buffer
	for _, e := range events {
		if err := json.NewEncoder(&out).Encode(e); err != nil {
			panic(err)
		}
	}
	return out.String()
}

func trace(pkg, test string) string {
	return encode(event{Action: "start", Package: pkg}, event{Action: "run", Package: pkg, Test: test}, event{Action: "pass", Package: pkg, Test: test}, event{Action: "pass", Package: pkg})
}

func TestCheckLifecycle(t *testing.T) {
	start := event{Action: "start", Package: "p"}
	run := event{Action: "run", Package: "p", Test: "TestOne"}
	pass := event{Action: "pass", Package: "p", Test: "TestOne"}
	terminal := event{Action: "pass", Package: "p"}
	for name, input := range map[string]string{
		"empty":                        "",
		"package_only":                 encode(start, terminal),
		"missing_package_start":        encode(run, pass, terminal),
		"missing_package_terminal":     encode(start, run, pass),
		"missing_test_run":             encode(start, pass, terminal),
		"missing_test_terminal":        encode(start, run, terminal),
		"duplicate_start":              encode(start, start, run, pass, terminal),
		"duplicate_run":                encode(start, run, run, pass, terminal),
		"duplicate_test_terminal":      encode(start, run, pass, pass, terminal),
		"duplicate_package_terminal":   encode(start, run, pass, terminal, terminal),
		"conflicting_package_terminal": encode(start, run, pass, terminal, event{Action: "fail", Package: "p"}),
		"failed_test":                  encode(start, run, event{Action: "fail", Package: "p", Test: "TestOne"}),
		"failed_package":               encode(start, run, pass, event{Action: "fail", Package: "p"}),
		"skipped_test":                 encode(start, run, event{Action: "skip", Package: "p", Test: "TestOne"}),
		"skipped_package_with_tests":   encode(start, run, pass, event{Action: "skip", Package: "p"}),
		"unexplained_package_skip":     trace("q", "TestTwo") + encode(start, event{Action: "skip", Package: "p"}),
		"unfinished_other_package":     trace("q", "TestTwo") + encode(start),
		"unknown_event":                encode(start, event{Action: "surprise", Package: "p"}),
		"output_without_run":           encode(start, event{Action: "output", Package: "p", Test: "TestOne"}),
		"invalid_pause":                encode(start, event{Action: "pause", Package: "p", Test: "TestOne"}),
		"invalid_cont":                 encode(start, run, event{Action: "cont", Package: "p", Test: "TestOne"}),
		"malformed":                    "not JSON\n",
		"oversized_event":              strings.Repeat("x", maxEventBytes+1),
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := Check(strings.NewReader(input), nil); err == nil {
				t.Fatal("invalid results accepted")
			}
		})
	}
	for name, input := range map[string]string{
		"slash_in_subtest": encode(start, run, event{Action: "run", Package: "p", Test: "TestOne/a/b"}, event{Action: "pass", Package: "p", Test: "TestOne/a/b"}, pass, terminal),
		"ordinary":         trace("p", "TestOne"),
		"two_packages":     trace("p", "TestOne") + trace("q", "TestOne"),
		"parallel":         encode(start, run, event{Action: "pause", Package: "p", Test: "TestOne"}, event{Action: "cont", Package: "p", Test: "TestOne"}, pass, terminal),
		"no_test_files":    trace("p", "TestOne") + encode(event{Action: "start", Package: "q"}, event{Action: "output", Package: "q", Output: "? q [no test files]\n"}, event{Action: "skip", Package: "q"}),
	} {
		t.Run(name, func(t *testing.T) {
			summary, err := Check(strings.NewReader(input), nil)
			if err != nil || summary.Tests == 0 || summary.Packages == 0 {
				t.Fatalf("valid results rejected: %+v; %v", summary, err)
			}
		})
	}
}

func integrationTrace(omit string) string {
	owners := IntegrationTests()
	pkg := owners[0].Package
	out := encode(event{Action: "start", Package: pkg})
	for _, test := range owners {
		if test.Name != omit {
			out += encode(event{Action: "run", Package: pkg, Test: test.Name}, event{Action: "pass", Package: pkg, Test: test.Name})
		}
	}
	return out + encode(event{Action: "pass", Package: pkg})
}

func TestIntegrationInventory(t *testing.T) {
	owners := IntegrationTests()
	complete := integrationTrace("")
	if summary, err := Check(strings.NewReader(complete), owners); err != nil || summary.Tests != len(owners) {
		t.Fatalf("complete integration failed: %+v; %v", summary, err)
	}
	for _, owner := range owners {
		t.Run("missing_"+owner.Name, func(t *testing.T) {
			if summary, err := Check(strings.NewReader(integrationTrace(owner.Name)), owners); err == nil || summary.FailureReason != "required_test_absent" {
				t.Fatalf("missing owner accepted: %v", err)
			}
		})
	}
	pkg := owners[0].Package
	first := owners[0].Name
	for name, input := range map[string]string{
		"old_package":      strings.ReplaceAll(complete, pkg, "github.com/Harness-X-Harness/codex/grok/live"),
		"parent_only":      trace(pkg, "TestIntegration"),
		"descendant_only":  strings.ReplaceAll(complete, first, first+"/child"),
		"skipped_owner":    strings.Replace(complete, `"Action":"pass","Package":"`+pkg+`","Test":"`+first+`"`, `"Action":"skip","Package":"`+pkg+`","Test":"`+first+`"`, 1),
		"failed_owner":     strings.Replace(complete, `"Action":"pass","Package":"`+pkg+`","Test":"`+first+`"`, `"Action":"fail","Package":"`+pkg+`","Test":"`+first+`"`, 1),
		"duplicate_owner":  strings.Replace(complete, encode(event{Action: "pass", Package: pkg, Test: first}), strings.Repeat(encode(event{Action: "pass", Package: pkg, Test: first}), 2), 1),
		"incomplete_owner": strings.Replace(complete, encode(event{Action: "pass", Package: pkg, Test: first}), "", 1),
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := Check(strings.NewReader(input), owners); err == nil {
				t.Fatal("invalid Integration owner evidence accepted")
			}
		})
	}
}

func TestIntegrationParentCannotReplaceProofDescendants(t *testing.T) {
	for _, parent := range []string{"TestStructuredEditFixture", "TestHostedSearchFixture", "TestFirstRequestShape"} {
		t.Run(parent, func(t *testing.T) {
			owners := IntegrationTests()
			pkg := owners[0].Package
			input := encode(event{Action: "start", Package: pkg})
			for _, owner := range owners {
				if !strings.HasPrefix(owner.Name, parent+"/") {
					input += encode(event{Action: "run", Package: pkg, Test: owner.Name}, event{Action: "pass", Package: pkg, Test: owner.Name})
				}
			}
			input += encode(event{Action: "pass", Package: pkg})
			if _, err := Check(strings.NewReader(input), owners); err == nil {
				t.Fatal("parent-only pass accepted without its proof-bearing scenarios")
			}
		})
	}
}

func TestIntegrationOwnerSelection(t *testing.T) {
	for _, names := range [][]string{
		nil, {""}, {"TestUnknown"}, {"TestBasicFixture/child"}, {"TestBasicFixture", "TestBasicFixture"},
	} {
		if _, err := IntegrationOwnerTests(names); err == nil {
			t.Fatalf("invalid owner selection accepted: %v", names)
		}
	}
	for _, parent := range []string{"TestBasicFixture", "TestStructuredEditFixture", "TestFirstRequestShape", "TestShippedChildFixture"} {
		t.Run(parent, func(t *testing.T) {
			selected, err := IntegrationOwnerTests([]string{parent})
			if err != nil {
				t.Fatal(err)
			}
			if len(selected) < 2 || selected[0].Name != parent {
				t.Fatal("selected owner lost its proof-bearing scenarios")
			}
			pkg := selected[0].Package
			complete := encode(event{Action: "start", Package: pkg})
			for _, test := range selected {
				if test.Name != parent && !strings.HasPrefix(test.Name, parent+"/") {
					t.Fatalf("unselected owner included: %s", test.Name)
				}
				complete += encode(event{Action: "run", Package: pkg, Test: test.Name}, event{Action: "pass", Package: pkg, Test: test.Name})
			}
			complete += encode(event{Action: "pass", Package: pkg})
			if _, err := Check(strings.NewReader(complete), selected); err != nil {
				t.Fatalf("complete selected scope rejected: %v", err)
			}
			for _, test := range selected {
				missing := strings.ReplaceAll(complete, encode(event{Action: "run", Package: pkg, Test: test.Name}, event{Action: "pass", Package: pkg, Test: test.Name}), "")
				if _, err := Check(strings.NewReader(missing), selected); err == nil {
					t.Fatalf("selected proof omitted without rejection: %s", test.Name)
				}
			}
		})
	}
	selected, err := IntegrationOwnerTests([]string{"TestShippedCatalog", "TestBasicFixture"})
	if err != nil || len(selected) != 4 {
		t.Fatalf("combined owner selection lost or added proofs: %v, %v", selected, err)
	}
}

func TestRequiredOwners(t *testing.T) {
	for _, required := range [][]Test{
		{{"p", "TestMissing"}}, {{"wrong", "TestOne"}}, {{"", "TestOne"}}, {{"p", ""}}, {{"p", "TestOne"}, {"p", "TestOne"}},
	} {
		if _, err := Check(strings.NewReader(trace("p", "TestOne")), required); err == nil {
			t.Fatal("invalid selection accepted")
		}
	}
	if _, err := Check(strings.NewReader(trace("p", "TestOne")), []Test{{"p", "TestOne"}}); err != nil {
		t.Fatal(err)
	}
}

type brokenReader struct{}

func (brokenReader) Read([]byte) (int, error) { return 0, errors.New("PRIVATE_CONTENT") }

func TestReadFailureIsBoundedAndPrivate(t *testing.T) {
	_, err := Check(io.MultiReader(strings.NewReader(trace("p", "TestOne")), brokenReader{}), nil)
	if err == nil || strings.Contains(err.Error(), "PRIVATE_CONTENT") {
		t.Fatal("read failure accepted or leaked")
	}
}

func TestSummaryCountsWholeFailedStream(t *testing.T) {
	failed := encode(event{Action: "start", Package: "bad"},
		event{Action: "run", Package: "bad", Test: "TestFailure"},
		event{Action: "fail", Package: "bad", Test: "TestFailure"},
		event{Action: "fail", Package: "bad"})
	skipped := encode(event{Action: "start", Package: "skipped"},
		event{Action: "run", Package: "skipped", Test: "TestSkip"},
		event{Action: "skip", Package: "skipped", Test: "TestSkip"},
		event{Action: "pass", Package: "skipped"})
	incomplete := encode(event{Action: "start", Package: "incomplete"},
		event{Action: "run", Package: "incomplete", Test: "TestWaiting"})
	for _, input := range []string{failed + trace("good", "TestPass") + skipped + incomplete,
		trace("good", "TestPass") + skipped + incomplete + failed} {
		got, err := Check(strings.NewReader(input), nil)
		if err == nil || !got.InputComplete || got.FailureReason != "failed_execution" {
			t.Fatalf("failure missing: %+v %v", got, err)
		}
		if got.TestsStarted != 4 || got.TestsPassed != 1 || got.TestsFailed != 1 || got.TestsSkipped != 1 || got.TestsIncomplete != 1 {
			t.Fatalf("test counts lost after failure: %+v", got)
		}
		if got.PackagesStarted != 4 || got.PackagesPassed != 2 || got.PackagesFailed != 1 || got.PackagesIncomplete != 1 || got.PackagesSkipped != 0 {
			t.Fatalf("package counts lost after failure: %+v", got)
		}
		if got.Tests != got.TestsPassed || got.Packages != 2 {
			t.Fatalf("legacy counters changed: %+v", got)
		}
	}
}

func TestSummaryMalformedInputReportsTrustedPrefix(t *testing.T) {
	prefix := trace("good", "TestPass") + encode(event{Action: "start", Package: "PRIVATE_PACKAGE"},
		event{Action: "run", Package: "PRIVATE_PACKAGE", Test: "PRIVATE_TEST"})
	got, err := Check(strings.NewReader(prefix+"PRIVATE_INVALID_JSON\n"+trace("later", "TestLater")), nil)
	if err == nil || got.InputComplete || got.FailureReason != "invalid_event" || got.TestsStarted != 2 || got.TestsPassed != 1 || got.TestsIncomplete != 1 || got.PackagesStarted != 2 || got.PackagesIncomplete != 1 {
		t.Fatalf("invalid stream counts: %+v %v", got, err)
	}
	raw, _ := json.Marshal(got)
	if strings.Contains(string(raw)+err.Error(), "PRIVATE_") {
		t.Fatal("private source reached summary")
	}
}

func TestSummaryIncompleteSkippedAndBuildFailure(t *testing.T) {
	for _, tc := range []struct {
		input, reason               string
		incomplete, skipped, failed int
	}{
		{encode(event{Action: "start", Package: "p"}, event{Action: "run", Package: "p", Test: "TestOne"}), "incomplete_test", 1, 0, 0},
		{encode(event{Action: "start", Package: "p"}, event{Action: "run", Package: "p", Test: "TestOne"}, event{Action: "skip", Package: "p", Test: "TestOne"}, event{Action: "pass", Package: "p"}) + trace("q", "TestTwo"), "skipped_execution", 0, 1, 0},
		{encode(event{Action: "start", Package: "p"}, event{Action: "build-fail", Package: "p"}) + trace("q", "TestTwo"), "failed_execution", 0, 0, 1},
	} {
		got, err := Check(strings.NewReader(tc.input), nil)
		if err == nil || !got.InputComplete || got.FailureReason != tc.reason || got.TestsIncomplete != tc.incomplete || got.TestsSkipped != tc.skipped || got.PackagesFailed != tc.failed {
			t.Fatalf("wrong terminal counts: %+v %v", got, err)
		}
	}
	got, err := Check(strings.NewReader(trace("p", "TestOne")+encode(event{Action: "start", Package: "empty"}, event{Action: "output", Package: "empty", Output: "[no test files]"}, event{Action: "skip", Package: "empty"})), nil)
	if err != nil || got.PackagesStarted != 2 || got.PackagesPassed != 1 || got.PackagesSkipped != 1 || got.TestsStarted != 1 {
		t.Fatalf("no-test package counts: %+v %v", got, err)
	}
}
