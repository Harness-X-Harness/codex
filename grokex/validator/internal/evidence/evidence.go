// Package evidence writes the secret-safe per-scenario evidence file that
// grokex/release.py composes into LIVE_EVIDENCE.json.
package evidence

import (
	"encoding/json"
	"fmt"
	"os"
	"runtime/debug"
	"strings"
	"time"

	"github.com/Harness-X-Harness/codex/grokex/validator/internal/oracle"
)

// Modes accepted on the command line.
const (
	ModeRelease     = "release"
	ModeObservation = "observation"
)

// Identity names the artifact, scenario, source, and run one evidence file
// describes. The run that produced it is the evidence; these fields make the
// file readable on its own.
type Identity struct {
	Archive       string
	ArchiveSHA256 string
	Mode          string
	Scenario      string
	Story         string
	SourceSHA     string
	ValidationRun string
}

// Clock records stage timings relative to validator start.
type Clock struct {
	start  time.Time
	stages []map[string]any
}

// NewClock starts the stage clock.
func NewClock() *Clock {
	return &Clock{start: time.Now()}
}

// Mark records that a stage was reached.
func (c *Clock) Mark(stage string) {
	c.stages = append(c.stages, map[string]any{
		"stage":           stage,
		"elapsed_seconds": roundSeconds(time.Since(c.start)),
	})
}

// Stages returns the recorded timeline.
func (c *Clock) Stages() []map[string]any {
	return c.stages
}

func roundSeconds(d time.Duration) float64 {
	return float64(d.Milliseconds()) / 1000
}

// Failure describes why a scenario did not reach GREEN.
type Failure struct {
	Category string
	Reason   string
	Stage    string
}

var outcomeByCategory = map[string]string{
	"deadline":          "deadline_expired",
	"semantic_contract": "semantic_failure",
	"delivery":          "delivery_failure",
	"harness":           "harness_failure",
	"environment":       "environment_failure",
}

// Document is the assembled evidence.
type Document struct {
	Identity    Identity
	Assertions  map[string]any
	Diagnostics map[string]any
	Stages      []map[string]any
	Failure     *Failure
}

// FromVerdict merges an oracle verdict into a document.
func FromVerdict(identity Identity, clock *Clock, verdict oracle.Verdict) Document {
	document := Document{
		Identity:    identity,
		Assertions:  verdict.Assertions,
		Diagnostics: verdict.Diagnostics,
		Stages:      clock.Stages(),
	}
	if !verdict.OK() {
		document.Failure = &Failure{Category: verdict.FailureCategory, Reason: verdict.Failure, Stage: verdict.LastProvenStage}
	}
	return document
}

// Fields flattens the document into the wire object. Assertions keep their
// exact keys because the composer checks them by value; diagnostics never
// shadow an assertion.
func (d Document) Fields() map[string]any {
	fields := map[string]any{
		"archive":            d.Identity.Archive,
		"archive_sha256":     d.Identity.ArchiveSHA256,
		"catalog":            "release-bundled",
		"mode":               d.Identity.Mode,
		"model":              "grok-4.6",
		"provider":           "grok",
		"scenario":           d.Identity.Scenario,
		"source_sha":         d.Identity.SourceSHA,
		"story":              d.Identity.Story,
		"validation_run":     d.Identity.ValidationRun,
		"validator":          "grokex/validator",
		"validator_protocol": protocolVersion(),
		"stage_timings":      d.Stages,
	}
	for key, value := range d.Diagnostics {
		if _, taken := fields[key]; !taken {
			fields[key] = value
		}
	}
	for key, value := range d.Assertions {
		fields[key] = value
	}
	if d.Failure != nil {
		outcome, ok := outcomeByCategory[d.Failure.Category]
		if !ok {
			outcome = "failed"
		}
		fields["outcome"] = outcome
		fields["failure_category"] = d.Failure.Category
		fields["failure_reason"] = d.Failure.Reason
		fields["last_proven_stage"] = d.Failure.Stage
		fields["does_not_prove"] = "product_root_cause"
		delete(fields, "status")
	}
	return fields
}

// Write persists the document as sorted, indented JSON.
func Write(path string, document Document) error {
	data, err := json.MarshalIndent(document.Fields(), "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(path, append(data, '\n'), 0o644)
}

// protocolVersion names the exact codexsdk module this binary was built with,
// so evidence records which generated protocol the validator spoke.
func protocolVersion() string {
	info, ok := debug.ReadBuildInfo()
	if !ok {
		return "codexsdk@unknown"
	}
	for _, dep := range info.Deps {
		if strings.HasSuffix(dep.Path, "/codexsdk") {
			return fmt.Sprintf("codexsdk@%s", dep.Version)
		}
	}
	return "codexsdk@unknown"
}
