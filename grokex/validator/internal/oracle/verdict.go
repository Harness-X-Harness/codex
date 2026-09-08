// Package oracle turns canonical session facts and delivered results into the
// per-scenario assertions grokex/release.py composes into LIVE_EVIDENCE.json.
//
// Every assertion is a documented Story invariant. Counts, tool names, and
// timings are diagnostics: they explain a failure, they never decide one.
package oracle

// EvidenceSource names the oracle so the composer can tell canonical-session
// evidence from stream-based evidence.
const EvidenceSource = "canonical_session"

// Verdict is the outcome of one scenario oracle.
type Verdict struct {
	// Assertions are the contract fields the composer checks by exact value.
	Assertions map[string]any
	// Diagnostics are secret-safe hints written next to the assertions.
	Diagnostics map[string]any
	// LastProvenStage is the furthest stage every earlier assertion held at.
	LastProvenStage string
	// Failure is empty on success, otherwise the first contract that did not hold.
	Failure string
	// FailureCategory classifies Failure: semantic_contract, deadline, or delivery.
	FailureCategory string
}

// OK reports whether every contract held.
func (v Verdict) OK() bool {
	return v.Failure == ""
}

type stages struct {
	verdict *Verdict
	failed  bool
}

func newVerdict() (*Verdict, *stages) {
	verdict := &Verdict{Assertions: map[string]any{}, Diagnostics: map[string]any{}}
	return verdict, &stages{verdict: verdict}
}

// require records a stage as proven when ok holds; the first failure freezes
// LastProvenStage and names the broken contract.
func (s *stages) require(stage string, ok bool, failure, category string) bool {
	if s.failed {
		return false
	}
	if !ok {
		s.failed = true
		s.verdict.Failure = failure
		s.verdict.FailureCategory = category
		return false
	}
	s.verdict.LastProvenStage = stage
	return true
}
