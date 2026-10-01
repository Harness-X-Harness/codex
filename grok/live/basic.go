// Package live observes explicit binaries through App Server's public protocol.
package live

import (
	"context"
	"errors"
)

// Subject's source/target provenance and native eligibility are caller-established.
// Basic verifies the executable digest; it does not infer provenance from labels.
type Subject struct {
	Binary, SHA256, SourceSHA, HarnessSHA, Target, Environment string
}

// Options selects an isolated harness fixture, independent of shipped assets.
type Options struct {
	Subject                Subject
	Model, BaseURL, APIKey string
}

// Evidence contains only safe metadata and observations, never private traffic.
type Evidence struct {
	SHA256, SourceSHA, HarnessSHA, Target, Environment, Model, ObservedAt, Stage string
	Processes, Initializations, Threads, Turns, ReplyBytes                       int
	Bound, Completed                                                             bool
}

// Basic initiates one process, initialization, thread and text turn without retry.
// A successful observation assumes the caller established Subject's prerequisites.
func Basic(ctx context.Context, options Options) (Evidence, error) {
	return Evidence{}, errors.New("live: Basic is not implemented")
}
