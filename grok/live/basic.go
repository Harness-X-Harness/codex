// Package live observes explicit binaries through App Server's public protocol.
package live

import (
	"context"
	"encoding/json"
	"errors"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/integrationfixture"
	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/internal/testkit"
)

// Subject's source/target provenance and native eligibility are caller-established.
// The runner verifies the executable digest; it does not infer provenance from labels.
type Subject struct {
	Binary, SHA256, SourceSHA, HarnessSHA, Target, Environment string
	// LocalUnverified marks developer observations with no asserted source provenance.
	LocalUnverified bool `json:",omitempty"`
}

// Options selects an isolated harness fixture, independent of shipped assets.
type Options struct {
	// Diagnostics is a separate task-scoped artifact sink; never an oracle.
	Diagnostics                        *Diagnostics
	Subject                            Subject
	Model, BaseURL, APIKey             string
	imageFixture, attachedImageFixture bool
	shippedProfile, catalogOnly        bool
}

// Evidence contains only safe metadata and observations, never private traffic.
type Evidence struct {
	// CleanupPending means SDK startup/teardown did not prove all owned work joined.
	CleanupPending                                                               bool `json:",omitempty"`
	LocalUnverified                                                              bool `json:",omitempty"`
	SetupTurns, TaskTurns                                                        int
	SetupCompleted                                                               bool
	CatalogModels                                                                int
	ShippedCatalog                                                               bool
	ChildBound, ChildCompleted, ChildResultDelivered                             bool
	SHA256, SourceSHA, HarnessSHA, Target, Environment, Model, ObservedAt, Stage string
	Processes, Initializations, Threads, Turns, ReplyBytes                       int
	Bound, Completed                                                             bool
	ReasoningItems, EncryptedItems, ToolCalls, CompletedTools, DeniedToolCalls   int
	FirstReplyBytes                                                              int
	FirstCompleted, FirstRecall, Recalled                                        bool
}

// withFixture verifies the explicit executable and isolates one workspace.
// Every scenario uses the pinned SDK; this setup does not speak App Server.
func withFixture(ctx context.Context, options Options, edit *editFixture, run func(home, cwd string, evidence *Evidence) error) (evidence Evidence, resultErr error) {
	defer func() {
		options.Diagnostics.Record("fixture_result", map[string]any{"stage": evidence.Stage, "completed": evidence.Completed, "cleanup_pending": evidence.CleanupPending, "failure": diagnosticFailure(resultErr)})
	}()
	evidence = Evidence{Stage: "preflight", ObservedAt: time.Now().UTC().Format(time.RFC3339)}
	deadline, bounded := ctx.Deadline()
	subject := options.Subject
	controlledEndpoint, controlled := integrationfixture.ShippedEndpoint(ctx)
	if controlled {
		options.BaseURL, options.shippedProfile = controlledEndpoint, true
	}
	endpoint, err := url.Parse(options.BaseURL)
	if !bounded || time.Until(deadline) <= 0 || time.Until(deadline) > 10*time.Minute ||
		!filepath.IsAbs(subject.Binary) || !digest.MatchString(subject.SHA256) ||
		!validSubjectProvenance(subject) ||
		!label.MatchString(subject.Target) || !label.MatchString(subject.Environment) ||
		(options.Model != providerfixture.PrimaryModel && options.Model != providerfixture.PinnedModel) ||
		strings.TrimSpace(options.APIKey) == "" || err != nil || endpoint.Host == "" ||
		(endpoint.Scheme != "http" && endpoint.Scheme != "https") || endpoint.User != nil ||
		endpoint.RawQuery != "" || endpoint.Fragment != "" {
		return evidence, errors.New("live: invalid subject, fixture or deadline")
	}
	evidence.LocalUnverified = subject.LocalUnverified
	evidence.SHA256, evidence.SourceSHA, evidence.HarnessSHA = subject.SHA256, subject.SourceSHA, subject.HarnessSHA
	evidence.Target, evidence.Environment, evidence.Model = subject.Target, subject.Environment, options.Model
	actualDigest, err := testkit.ExecutableSHA256(subject.Binary)
	if err != nil {
		return evidence, errors.New("live: executable unavailable or invalid")
	}
	if actualDigest != subject.SHA256 {
		return evidence, errors.New("live: executable digest mismatch")
	}
	// Check the path after the bounded SDK cleanup attempt. CleanupPending
	// separately records whether process/callback termination was proved.
	defer func() {
		finalDigest, err := testkit.ExecutableSHA256(subject.Binary)
		if err != nil || finalDigest != subject.SHA256 {
			evidence.Completed = false
			evidence.Stage = "artifact_changed"
			resultErr = errors.Join(resultErr, errors.New("live: executable changed during observation"))
		}
	}()
	evidence.Stage = "artifact_verified"
	home, err := os.MkdirTemp("", "grok-live-")
	if err != nil {
		return evidence, errors.New("live: isolated fixture unavailable")
	}
	defer func() {
		if !evidence.CleanupPending {
			_ = os.RemoveAll(home)
		}
	}()
	cwd := filepath.Join(home, "workspace")
	if err := writeFixture(home, cwd, options, edit); err != nil {
		return evidence, err
	}
	if controlled {
		if err := integrationfixture.SubstituteShippedEndpoint(home, controlledEndpoint); err != nil {
			return evidence, err
		}
	}
	evidence.Stage = "fixture_ready"
	err = run(home, cwd, &evidence)
	return evidence, err
}

func validSubjectProvenance(subject Subject) bool {
	if subject.LocalUnverified {
		return subject.SourceSHA == "" && subject.HarnessSHA == ""
	}
	return revision.MatchString(subject.SourceSHA) && revision.MatchString(subject.HarnessSHA)
}

var (
	digest   = regexp.MustCompile(`^[0-9a-f]{64}$`)
	revision = regexp.MustCompile(`^[0-9a-f]{40}$`)
	label    = regexp.MustCompile(`^[a-zA-Z0-9_.-]{1,64}$`)
)

type item struct {
	Type  string `json:"type"`
	Phase string `json:"phase"`
	Text  string `json:"text"`
}
type turn struct {
	ID     string          `json:"id"`
	Status string          `json:"status"`
	Error  json.RawMessage `json:"error"`
	Items  []item          `json:"items"`
}
