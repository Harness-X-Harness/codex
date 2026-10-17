//go:build facts

package facts_test

import "testing"

// TestBackendSearchFacts observes provider-only Web/X admission and replay.
// X remains useful backend evidence even when the Codex product retires X.
// Policy admission alone establishes neither a hosted call nor enforcement.
func TestBackendSearchFacts(t *testing.T) {
	for _, scenario := range cases {
		if searchFact(scenario) {
			t.Run(scenario.Name, func(t *testing.T) { observe(t, scenario) })
		}
	}
}
