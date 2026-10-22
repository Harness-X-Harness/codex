package scenarios

import (
	"context"
	"errors"
	"reflect"
	"testing"

	"github.com/Harness-X-Harness/codex/grok/facts"
	"github.com/Harness-X-Harness/codex/grok/live"
)

func TestSuiteSelectionPreservesFormalAndDiagnosticBoundaries(t *testing.T) {
	full, err := Select("live", "full", "")
	if err != nil {
		t.Fatal(err)
	}
	diagnostic, err := Select("live", "diagnostic", "")
	if err != nil {
		t.Fatal(err)
	}
	smoke, err := Select("live", "smoke", "")
	if err != nil {
		t.Fatal(err)
	}
	var smokeNames []string
	for _, scenario := range smoke {
		smokeNames = append(smokeNames, scenario.Name)
	}
	if !reflect.DeepEqual(smokeNames, []string{"basic-primary", "reasoning-primary", "web"}) {
		t.Fatalf("unexpected smoke selection: %v", smokeNames)
	}
	if len(full) != 18 || len(diagnostic) != len(full) {
		t.Fatal("formal proof silently changed")
	}
	for _, entry := range full {
		selected, err := Select("live", "full", entry.Name)
		if !entry.Formal || err != nil || len(selected) != 1 || selected[0].Name != entry.Name {
			t.Fatal("single-name selection disagrees with full suite")
		}
	}
	facts, err := Select("facts", "full", "")
	if err != nil || len(facts) != 11 {
		t.Fatal("independent Facts coverage changed")
	}
	for _, entry := range facts {
		if entry.RequiresBinary {
			t.Fatal("Facts acquired a product binary requirement")
		}
	}
}

func TestSelectionRejectsUnknownExcludedAndEmptyCatalog(t *testing.T) {
	for _, args := range [][3]string{{"", "full", ""}, {"live", "", ""}, {"live", "full", "stale"}, {"facts", "full", "reasoning-primary"}, {"live", "smoke", "basic-pinned"}} {
		if selected, err := Select(args[0], args[1], args[2]); err == nil || len(selected) != 0 {
			t.Fatal("invalid selection succeeded")
		}
	}
	if _, err := Lookup("live", ""); err == nil {
		t.Fatal("missing exact name succeeded")
	}
	if err := validate(nil); err == nil {
		t.Fatal("empty catalog accepted")
	}
	if err := validate([]Scenario{catalog[0], catalog[0]}); err == nil {
		t.Fatal("duplicate dispatch accepted")
	}
	bad := catalog[0]
	bad.Name = ""
	if err := validate([]Scenario{bad}); err == nil {
		t.Fatal("empty name accepted")
	}
	bad = catalog[0]
	bad.liveRun = nil
	if err := validate([]Scenario{bad}); err == nil {
		t.Fatal("metadata without active runner accepted")
	}
}

func TestDispatchUsesOwnedModelAndDoesNotRetryFailures(t *testing.T) {
	calls := 0
	failure := errors.New("owned failure")
	s := liveCase("owned", "owned-model", "text", 1, false, func(_ context.Context, options live.Options) (LiveResult, error) {
		calls++
		if options.Model != "owned-model" {
			t.Error("caller overrode catalog model")
		}
		return LiveResult{Base: live.Evidence{Turns: 1}}, failure
	})
	result, err := s.RunLive(context.Background(), live.Options{Model: "other"})
	if err != failure || calls != 1 || result.Base.Turns != 1 {
		t.Fatal("failure was lost or retried")
	}
	if _, err := s.RunFacts(context.Background(), &facts.Probe{}); err == nil {
		t.Fatal("Live became direct Facts")
	}
	factsCase := factsCase("owned", "owned-model", "text", 1, false, func(context.Context, *facts.Probe) (FactsResult, error) {
		calls++
		return FactsResult{Observation: &facts.Observation{Requests: 1}}, failure
	})
	observed, err := factsCase.RunFacts(context.Background(), &facts.Probe{})
	if err != failure || calls != 2 || observed.Completed() {
		t.Fatal("Facts failure was lost or retried")
	}
	if _, err := factsCase.RunLive(context.Background(), live.Options{}); err == nil {
		t.Fatal("Facts became product Live")
	}
}

func TestFactsCompletionRequiresEveryRoute(t *testing.T) {
	result := FactsResult{Routes: []facts.RouteObservation{{Observation: facts.Observation{Completed: true}}, {Observation: facts.Observation{Requests: 1}}}}
	if result.Completed() || result.LastObservation().Requests != 1 {
		t.Fatal("partial routes became completed")
	}
	result.Routes[1].Observation.Completed = true
	if !result.Completed() {
		t.Fatal("completed route set rejected")
	}
}

func TestRetiredProductXDoesNotRetireBackendFacts(t *testing.T) {
	for _, name := range []string{"x", "x-window"} {
		for _, suite := range []string{"smoke", "full", "diagnostic"} {
			if selected, err := Select("live", suite, name); err == nil || len(selected) != 0 {
				t.Fatal("retired product X selected")
			}
		}
		if _, err := Lookup("facts", name); err != nil {
			t.Fatal("backend-only X Facts lost")
		}
	}
	if _, err := Lookup("facts", "x-replay"); err != nil {
		t.Fatal("backend X replay lost")
	}
}
