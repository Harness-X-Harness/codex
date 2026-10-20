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
			if _, err := Check(strings.NewReader(integrationTrace(owner.Name)), owners); err == nil || !strings.Contains(err.Error(), owner.Name) {
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
