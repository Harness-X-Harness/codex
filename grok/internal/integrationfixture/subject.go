package integrationfixture

import (
	"errors"
	"regexp"
	"runtime"

	"github.com/Harness-X-Harness/codex/grok/internal/testkit"
)

// Subject is the selected executable identity and owned snapshot for controlled tests.
type Subject struct {
	Binary, SHA256, SourceSHA, HarnessSHA, Target, Environment string
	LocalUnverified                                            bool
}

// SelectSubject snapshots an explicitly selected executable and returns cleanup
// for the caller to invoke only after all process and callback work has joined.
func SelectSubject(getenv func(string) string) (Subject, func(), error) {
	binary, err := testkit.SelectBinary(getenv)
	if err != nil {
		return Subject{}, nil, err
	}
	digest, err := testkit.ExecutableSHA256(binary)
	if err != nil {
		return Subject{}, nil, err
	}
	subject := Subject{Binary: binary, SHA256: digest, Target: runtime.GOOS + "-" + runtime.GOARCH, Environment: "local-integration", LocalUnverified: true}
	source, harness := getenv("GROK_LIVE_NATIVE_SOURCE_SHA"), getenv("GITHUB_SHA")
	if source == "" {
		source = harness
	}
	if source != "" || harness != "" {
		if !sourceRevision.MatchString(source) || !sourceRevision.MatchString(harness) {
			return Subject{}, nil, errors.New("verified integration requires full source and harness revisions")
		}
		subject.SourceSHA, subject.HarnessSHA, subject.LocalUnverified = source, harness, false
		subject.Target, subject.Environment = "x86_64-unknown-linux-gnu", "native-fixture"

	}
	if target := getenv("GROK_LIVE_NATIVE_TARGET"); target != "" {
		subject.Target = target
		if !subject.LocalUnverified {
			subject.Environment = "package-fixture"
		}
	}
	// The selected source may be rebuilt while this test runs. Execute only
	// the owned byte-verified copy, retaining its original subject identity.
	snapshot, cleanup, err := testkit.SnapshotExecutable(binary, digest)
	if err != nil {
		return Subject{}, nil, err
	}
	subject.Binary = snapshot
	return subject, cleanup, nil
}

var sourceRevision = regexp.MustCompile(`^[0-9a-f]{40}$`)
