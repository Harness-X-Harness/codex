// Package live observes explicit binaries through App Server's public protocol.
package live

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
)

// Subject's source/target provenance and native eligibility are caller-established.
// The runner verifies the executable digest; it does not infer provenance from labels.
type Subject struct {
	Binary, SHA256, SourceSHA, HarnessSHA, Target, Environment string
}

// Options selects an isolated harness fixture, independent of shipped assets.
type Options struct {
	Subject                     Subject
	Model, BaseURL, APIKey      string
	imageFixture                bool
	shippedProfile, catalogOnly bool
	fixtureReady                func(home string) error
}

// Evidence contains only safe metadata and observations, never private traffic.
type Evidence struct {
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

// withFixture verifies the explicit executable and owns one isolated workspace.
// SDK and transitional legacy clients share setup, never protocol routing.
func withFixture(ctx context.Context, options Options, edit *editFixture, run func(home, cwd string, evidence *Evidence) error) (Evidence, error) {
	evidence := Evidence{Stage: "preflight", ObservedAt: time.Now().UTC().Format(time.RFC3339)}
	deadline, bounded := ctx.Deadline()
	subject := options.Subject
	endpoint, err := url.Parse(options.BaseURL)
	if !bounded || time.Until(deadline) <= 0 || time.Until(deadline) > 10*time.Minute ||
		!filepath.IsAbs(subject.Binary) || !digest.MatchString(subject.SHA256) ||
		!revision.MatchString(subject.SourceSHA) || !revision.MatchString(subject.HarnessSHA) ||
		!label.MatchString(subject.Target) || !label.MatchString(subject.Environment) ||
		(options.Model != providerfixture.PrimaryModel && options.Model != providerfixture.PinnedModel) ||
		strings.TrimSpace(options.APIKey) == "" || err != nil || endpoint.Host == "" ||
		(endpoint.Scheme != "http" && endpoint.Scheme != "https") || endpoint.User != nil ||
		endpoint.RawQuery != "" || endpoint.Fragment != "" {
		return evidence, errors.New("live: invalid subject, fixture or deadline")
	}
	evidence.SHA256, evidence.SourceSHA, evidence.HarnessSHA = subject.SHA256, subject.SourceSHA, subject.HarnessSHA
	evidence.Target, evidence.Environment, evidence.Model = subject.Target, subject.Environment, options.Model
	file, err := os.Open(subject.Binary)
	if err != nil {
		return evidence, errors.New("live: executable unavailable")
	}
	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() || info.Size() <= 0 || info.Size() > 1<<30 {
		_ = file.Close()
		return evidence, errors.New("live: invalid executable")
	}
	hash := sha256.New()
	_, err = io.Copy(hash, io.LimitReader(file, (1<<30)+1))
	_ = file.Close()
	if err != nil || hex.EncodeToString(hash.Sum(nil)) != subject.SHA256 {
		return evidence, errors.New("live: executable digest mismatch")
	}
	evidence.Stage = "artifact_verified"
	home, err := os.MkdirTemp("", "grok-live-")
	if err != nil {
		return evidence, errors.New("live: isolated fixture unavailable")
	}
	defer os.RemoveAll(home)
	cwd := filepath.Join(home, "workspace")
	if err := writeFixture(home, cwd, options, edit); err != nil {
		return evidence, err
	}
	if options.fixtureReady != nil {
		if err := options.fixtureReady(home); err != nil {
			return evidence, err
		}
	}
	evidence.Stage = "fixture_ready"
	err = run(home, cwd, &evidence)
	return evidence, err
}

// runFixture keeps unmigrated families on the legacy client until their owner lands.
func runFixture(ctx context.Context, options Options, edit *editFixture, threadOptions map[string]any, observe func(*appServer, string, *Evidence) error) (Evidence, error) {
	return withFixture(ctx, options, edit, func(home, cwd string, evidence *Evidence) error {
		server, err := startServer(ctx, options.Subject.Binary, home, cwd, options.APIKey)
		if err != nil {
			return err
		}
		defer server.close()
		evidence.Processes, evidence.Stage = 1, "process_started"
		evidence.Initializations = 1
		var initialized struct {
			UserAgent string `json:"userAgent"`
		}
		if err := server.call("initialize", map[string]any{"clientInfo": map[string]any{"name": "grok-live", "version": "1"}, "capabilities": map[string]any{"experimentalApi": true}}, &initialized); err != nil {
			return err
		}
		if initialized.UserAgent == "" {
			return errors.New("live: initialization evidence unavailable")
		}
		if err := server.send(frame{Method: "initialized"}); err != nil {
			return err
		}
		if options.shippedProfile {
			if err := requireShippedCatalog(server, evidence); err != nil {
				return err
			}
			if options.catalogOnly {
				return nil
			}
		}
		evidence.Stage, evidence.Threads = "initialized", 1
		var thread struct {
			Model    string `json:"model"`
			Provider string `json:"modelProvider"`
			Thread   struct {
				ID       string `json:"id"`
				Provider string `json:"modelProvider"`
			} `json:"thread"`
		}
		threadParams := map[string]any{"model": options.Model, "modelProvider": "grok", "cwd": cwd}
		for key, value := range threadOptions {
			threadParams[key] = value
		}
		if err := server.call("thread/start", threadParams, &thread); err != nil {
			return err
		}
		if thread.Thread.ID == "" || thread.Model != options.Model || thread.Provider != "grok" || thread.Thread.Provider != "grok" {
			return errors.New("live: fixture binding not established")
		}
		evidence.Bound, evidence.Stage = true, "thread_bound"
		return observe(server, thread.Thread.ID, evidence)
	})
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
