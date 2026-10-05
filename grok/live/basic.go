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

// Basic initiates one process, initialization, thread and text turn without retry.
// A successful observation assumes the caller established Subject's prerequisites.
func Basic(ctx context.Context, options Options) (Evidence, error) {
	return runFixture(ctx, options, nil, nil, observeBasic)
}

// runFixture owns the shared artifact, process, initialization and thread binding.
func runFixture(ctx context.Context, options Options, edit *editFixture, threadOptions map[string]any, observe func(*appServer, string, *Evidence) error) (Evidence, error) {
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
	evidence.Stage = "fixture_ready"
	server, err := startServer(ctx, subject.Binary, home, cwd, options.APIKey)
	if err != nil {
		return evidence, err
	}
	defer server.close()
	evidence.Processes, evidence.Stage = 1, "process_started"
	evidence.Initializations = 1
	var initialized struct {
		UserAgent string `json:"userAgent"`
	}
	if err := server.call("initialize", map[string]any{"clientInfo": map[string]any{"name": "grok-live", "version": "1"}, "capabilities": map[string]any{"experimentalApi": true}}, &initialized); err != nil {
		return evidence, err
	}
	if initialized.UserAgent == "" {
		return evidence, errors.New("live: initialization evidence unavailable")
	}
	if err := server.send(frame{Method: "initialized"}); err != nil {
		return evidence, err
	}
	if options.shippedProfile {
		if err := requireShippedCatalog(server, &evidence); err != nil {
			return evidence, err
		}
		if options.catalogOnly {
			return evidence, nil
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
		return evidence, err
	}
	if thread.Thread.ID == "" || thread.Model != options.Model || thread.Provider != "grok" || thread.Thread.Provider != "grok" {
		return evidence, errors.New("live: fixture binding not established")
	}
	evidence.Bound, evidence.Stage = true, "thread_bound"
	err = observe(server, thread.Thread.ID, &evidence)
	return evidence, err
}

func observeBasic(server *appServer, threadID string, evidence *Evidence) error {
	evidence.Turns = 1
	var started struct {
		Turn turn `json:"turn"`
	}
	if err := server.call("turn/start", map[string]any{"threadId": threadID, "input": []any{map[string]any{"type": "text", "text": "Reply with a short confirmation that this turn completed.", "textElements": []any{}}}}, &started); err != nil {
		return err
	}
	if started.Turn.ID == "" {
		return errors.New("live: turn identity unavailable")
	}
	evidence.Stage = "turn_submitted"
	observeReply := func(item item) {
		if item.Type == "agentMessage" && (item.Phase == "" || item.Phase == "final_answer") && strings.TrimSpace(item.Text) != "" {
			evidence.ReplyBytes = len(item.Text)
			if !evidence.Completed {
				evidence.Stage = "reply_observed"
			}
		}
	}
	observeTurn := func(turn turn) error {
		if turn.Status != "completed" || len(turn.Error) != 0 && string(turn.Error) != "null" {
			return errors.New("live: turn did not complete successfully")
		}
		evidence.Completed, evidence.Stage = true, "turn_completed"
		for _, item := range turn.Items {
			observeReply(item)
		}
		return nil
	}
	if started.Turn.Status != "inProgress" {
		if err := observeTurn(started.Turn); err != nil {
			return err
		}
	}
	for !evidence.Completed || evidence.ReplyBytes == 0 {
		message, err := server.next()
		if err != nil {
			return err
		}
		if len(message.ID) != 0 {
			return server.refuse(message.ID)
		}
		if message.Method != "turn/completed" && message.Method != "item/completed" {
			continue
		}
		var completed struct {
			ThreadID string `json:"threadId"`
			TurnID   string `json:"turnId"`
			Turn     turn   `json:"turn"`
			Item     item   `json:"item"`
		}
		if json.Unmarshal(message.Params, &completed) != nil {
			return errors.New("live: invalid completion evidence")
		}
		if completed.ThreadID != threadID {
			continue
		}
		if message.Method == "item/completed" {
			if completed.TurnID != started.Turn.ID {
				continue
			}
			observeReply(completed.Item)
		} else {
			if completed.Turn.ID != started.Turn.ID {
				continue
			}
			if err := observeTurn(completed.Turn); err != nil {
				return err
			}
		}
	}
	evidence.Stage = "final_reply"
	return nil
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
