//go:build facts

package facts_test

import "testing"

// TestBackendFacts owns direct text, encrypted replay and shipped-route probes.
// Their observations never establish a Codex product Live Story.
func TestBackendFacts(t *testing.T) {
	for _, scenario := range cases {
		if !searchFact(scenario) {
			t.Run(scenario.Name, func(t *testing.T) { observe(t, scenario) })
		}
	}
}
