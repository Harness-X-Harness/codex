package live

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

const (
	editFile = "store-hours.txt"
	editOld  = "The shop opens at 8 AM."
	editNew  = "The shop opens at 9 AM."
)

type editFixture struct {
	path, seed, expected              string
	replaceAll, decline, continuation bool
}

// StructuredEditEvidence reports scoped observations, not packaged Story success.
// In particular, App Server output cannot prove outbound HTTP history replay.
type StructuredEditEvidence struct {
	Evidence
	// A matching successful terminal was observed in the current turn. Later
	// evidence failures preserve this fact; Completed requires the whole scenario.
	TurnCompleted                     bool
	FileChanges, Approvals            int
	BeforeSHA256, AfterSHA256         string
	BytesMatch, ContinuationUnchanged bool
}

// StructuredEdit observes both requested replacements and one same-thread continuation.
func StructuredEdit(ctx context.Context, options Options) (StructuredEditEvidence, error) {
	return runStructuredEdit(ctx, options, editFixture{replaceAll: true, continuation: true})
}

// StructuredEditApprovalDeclined declines file-change approvals and requires no mutation.
func StructuredEditApprovalDeclined(ctx context.Context, options Options) (StructuredEditEvidence, error) {
	return runStructuredEdit(ctx, options, editFixture{decline: true})
}

// StructuredEditPinnedPreviousModel exercises the pinned harness model once.
func StructuredEditPinnedPreviousModel(ctx context.Context, options Options) (StructuredEditEvidence, error) {
	options.Model = providerfixture.PinnedModel
	return runStructuredEdit(ctx, options, editFixture{})
}

// StructuredEditReplaceAll requires every seeded occurrence to be replaced.
func StructuredEditReplaceAll(ctx context.Context, options Options) (StructuredEditEvidence, error) {
	return runStructuredEdit(ctx, options, editFixture{replaceAll: true})
}

func runStructuredEdit(ctx context.Context, options Options, fixture editFixture) (StructuredEditEvidence, error) {
	fixture.seed = "Shop notice\r\n" + editOld + "\r\nThank you."
	if fixture.replaceAll {
		fixture.seed += "\r\n" + editOld
	}
	fixture.expected = strings.ReplaceAll(fixture.seed, editOld, editNew)
	if fixture.decline {
		fixture.expected = fixture.seed
	}
	observer := newSDKEditObserver(&fixture)
	observer.diagnostics = options.Diagnostics
	var evidence StructuredEditEvidence
	checkBytes := func(expected string) error {
		data, err := readEditFixture(fixture.path)
		if err != nil {
			return err
		}
		evidence.AfterSHA256 = editDigest(data)
		evidence.BytesMatch = string(data) == expected
		if !evidence.BytesMatch {
			return failedObservation("edit_bytes_mismatch", "live: edit fixture bytes did not match")
		}
		return nil
	}
	handlers := sdkFixtureHandlers{
		requests: observer.request, notifications: observer.typedNotification, failures: observer.failures,
		retention: codexsdk.NotificationRetentionBudget{MaxCount: 4096, MaxBytes: 8 << 20},
	}
	base, err := withSDKFixture(ctx, options, &fixture, handlers, func(client *codexsdk.Client, cwd string, base *Evidence, closeSDK func() error) (runErr error) {
		// A failed task still joins owned callbacks before retaining partial facts.
		defer func() {
			if finalErr := closeSDK(); finalErr != nil {
				runErr = preferObservation(runErr, finalErr)
			}
			if base.CleanupPending {
				base.Completed, evidence.BytesMatch, evidence.ContinuationUnchanged = false, false, false
				return
			}
			captureEditDiagnostic(options.Diagnostics, fixture, "settled")
			snapshot, observationErr := observer.snapshot()
			base.Turns, base.FirstCompleted, base.FirstReplyBytes = snapshot.Turns, snapshot.FirstCompleted, snapshot.FirstReplyBytes
			base.ReplyBytes = snapshot.ReplyBytes
			evidence.TurnCompleted = snapshot.TurnCompleted
			evidence.FileChanges, evidence.Approvals = snapshot.FileChanges, snapshot.Approvals
			if finalErr := observationErr; finalErr != nil {
				runErr = preferObservation(runErr, finalErr)
			}
			if runErr != nil {
				base.Completed, evidence.BytesMatch, evidence.ContinuationUnchanged = false, false, false
				if snapshot.Stage != "" {
					base.Stage = snapshot.Stage
				}
				runErr = observer.failure(runErr)
			}
		}()

		seed, err := readEditFixture(fixture.path)
		if err != nil {
			return err
		}
		captureEditDiagnostic(options.Diagnostics, fixture, "before")
		evidence.BeforeSHA256 = editDigest(seed)
		if string(seed) != fixture.seed {
			return failedObservation("edit_bytes_mismatch", "live: edit fixture seed did not match")
		}
		policy := protocolv2.NewAskForApprovalNever()
		if fixture.decline {
			policy = protocolv2.NewAskForApprovalUntrusted()
		}
		fallback := false
		base.Threads = 1
		started, err := client.Threads().Start(ctx, protocolv2.ThreadStartParams{
			Model: protocolv2.Value(options.Model), ModelProvider: protocolv2.Value("grok"), CWD: protocolv2.Value(cwd),
			AllowProviderModelFallback: &fallback, ApprovalPolicy: protocolv2.Value(policy),
		})
		if err != nil {
			return sdkEditCause(ctx, observer, err)
		}
		sandbox, writable := started.Sandbox.AsWorkspaceWrite()
		if started.Model != options.Model || started.ModelProvider != "grok" || started.Thread.ModelProvider != "grok" ||
			started.Thread.ID == "" || started.CWD != cwd || started.Thread.CWD != cwd || started.ApprovalPolicy.Kind() != policy.Kind() ||
			!writable || sandbox.NetworkAccess != nil && *sandbox.NetworkAccess {
			return failedObservation("fixture_binding_failed", "live: fixture binding not established")
		}
		if sandbox.WritableRoots != nil {
			for _, root := range *sandbox.WritableRoots {
				if filepath.Clean(root) != cwd {
					return failedObservation("fixture_binding_failed", "live: fixture writable roots not bound")
				}
			}
		}
		if err := observer.bindThread(started.Thread.ID); err != nil {
			return err
		}
		base.Bound, base.Stage = true, "thread_bound"
		prompt := fmt.Sprintf("In %s, replace %q with %q. Preserve all other file content.", editFile, editOld, editNew)
		if fixture.replaceAll {
			prompt += " Replace both occurrences."
		}
		prompts := []string{prompt}
		if fixture.continuation {
			prompts = append(prompts, "Check that the requested edit is complete and confirm the result. Leave the file content unchanged.")
		}
		for index, prompt := range prompts {
			options.Diagnostics.Record("turn_input", map[string]any{"scenario": "edit", "turn": index + 1, "model": options.Model, "prompt": prompt, "decline": fixture.decline})
			evidence.BytesMatch, evidence.AfterSHA256 = false, ""
			if err := observer.beginTurn(); err != nil {
				return err
			}
			turn, err := client.Turns().Start(ctx, protocolv2.TurnStartParams{
				ThreadID: started.Thread.ID, Input: []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{Text: prompt})},
			})
			if err != nil {
				return sdkEditCause(ctx, observer, err)
			}
			if err := observer.reconcile(turn.Turn); err != nil {
				return err
			}
			if err := waitSDKEditTurn(ctx, observer); err != nil {
				return err
			}
			includeTurns := true
			settled, err := client.Threads().Read(ctx, protocolv2.ThreadReadParams{ThreadID: started.Thread.ID, IncludeTurns: &includeTurns})
			if err != nil {
				return sdkEditCause(ctx, observer, err)
			}
			// thread/read returns the platform's own cwd spelling. The fixture bytes below are the effect readback.
			if settled.Thread.ModelProvider != "grok" || settled.Thread.ID == "" {
				return failedObservation("fixture_binding_failed", "live: settled fixture binding not established")
			}
			if err := observer.validateSettled(settled.Thread.ID, settled.Thread.Turns); err != nil {
				return err
			}
			if err := observer.requireTurnProof(); err != nil {
				return err
			}
			captureEditDiagnostic(options.Diagnostics, fixture, fmt.Sprintf("turn-%d", index+1))
			if err := checkBytes(fixture.expected); err != nil {
				return err
			}
		}
		// No callback may still change the accepted result or the file after this fence.
		if err := closeSDK(); err != nil {
			return err
		}
		if err := observer.requireTurnProof(); err != nil {
			return err
		}
		if err := checkBytes(fixture.expected); err != nil {
			return err
		}
		base.Completed, evidence.BytesMatch, evidence.ContinuationUnchanged = true, true, fixture.continuation
		base.Stage = "edit_observed"
		if fixture.continuation {
			base.Stage = "edit_continuation_observed"
		}

		return nil
	})
	evidence.Evidence = base
	return evidence, err
}

type editProbe struct {
	turnID, previousID string
	fixture            *editFixture
	evidence           *StructuredEditEvidence
	base               *Evidence
}

// editItemFact retains only facts used by edit assertions and identity conflict checks.
// Paths, diffs and auxiliary SDK fields are deliberately not retained or hashed.
type editItemFact struct {
	ID     string `json:"id"`
	Type   string `json:"type"`
	Status string `json:"status"`
	Phase  string `json:"phase"`
	Text   string `json:"text"`
}

type editObservation struct {
	completed  bool
	replyBytes int
	replies    []protocolv2.ThreadItem
}

func readEditFixture(path string) ([]byte, error) {
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || info.Size() > 4096 {
		return nil, errors.New("live: edit fixture is not a bounded regular file")
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, errors.New("live: edit fixture unavailable")
	}
	opened, statErr := file.Stat()
	if statErr != nil || !os.SameFile(info, opened) {
		_ = file.Close()
		return nil, errors.New("live: edit fixture identity changed")
	}
	data, err := io.ReadAll(io.LimitReader(file, 4097))
	_ = file.Close()
	if err != nil || len(data) > 4096 {
		return nil, errors.New("live: edit fixture unavailable or oversized")
	}
	return data, nil
}

func (probe *editProbe) terminal(turn protocolv2.Turn, observation *editObservation) error {
	if turn.ID != probe.turnID {
		return errors.New("live: edit turn did not complete successfully")
	}
	if turn.Status != protocolv2.TurnStatusCompleted || turn.Error != nil && turn.Error.Value != nil {
		raw, _ := json.Marshal(turn.Error)
		return failedTurn(string(turn.Status), raw, "live: edit turn did not complete successfully")
	}
	observation.completed = true
	probe.evidence.TurnCompleted = true
	probe.base.Stage = "edit_continuation_completed"
	if probe.previousID == "" {
		probe.base.FirstCompleted, probe.base.Stage = true, "edit_turn_completed"
	}
	for _, item := range turn.Items {
		if err := probe.publicItem(editFact(item), observation); err != nil {
			return err
		}
	}
	return nil
}

func (probe *editProbe) publicItem(item editItemFact, observation *editObservation) error {
	switch item.Type {
	case "agentMessage":
		if item.ID == "" {
			return failedObservation("invalid_edit_evidence", "live: edit reply identity unavailable")
		}
		if item.Phase != "" && item.Phase != "final_answer" {
			return nil
		}
		message := protocolv2.ThreadItemAgentMessage{ID: item.ID, Text: item.Text}
		if item.Phase != "" {
			message.Phase = protocolv2.Value(protocolv2.MessagePhase(item.Phase))
		}
		observation.replies = append(observation.replies, protocolv2.NewThreadItemAgentMessage(message))
	case "fileChange":
		probe.evidence.FileChanges++
		if probe.fixture.decline && item.Status == "completed" {
			return failedObservation("edit_decline_violated", "live: file change completed after approval was declined")
		}

	}
	return nil
}

func editDigest(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

// editFact selects semantic evidence directly from the decoded SDK union.
func editFact(item protocolv2.ThreadItem) editItemFact {
	if value, ok := item.AsAgentMessage(); ok {
		fact := editItemFact{ID: value.ID, Type: "agentMessage", Text: value.Text}
		if value.Phase != nil && value.Phase.Value != nil {
			fact.Phase = string(*value.Phase.Value)
		}
		return fact
	}
	if value, ok := item.AsFileChange(); ok {
		return editItemFact{ID: value.ID, Type: "fileChange", Status: string(value.Status)}
	}
	return editItemFact{}
}

func sdkEditCause(ctx context.Context, observer *sdkEditObserver, err error) error {
	if ctx.Err() != nil {
		return observer.failure(ctx.Err())
	}
	return observer.failure(err)
}

func waitSDKEditTurn(ctx context.Context, observer *sdkEditObserver) error {
	for {
		observer.mu.Lock()
		cause := observer.cause
		ledger := observer.ledgers[len(observer.ledgers)-1]
		// A callback terminal is the serial notification prefix. An inline
		// start terminal still has to wait for its public reply and any denial evidence.
		ready := ledger.callbackTerminal || ledger.inlineTerminal && ledger.proof() == nil
		clientDone := observer.clientDone
		observer.mu.Unlock()
		if cause != nil {
			return cause
		}
		if ready {
			return observer.requireTurnProof()
		}
		select {
		case <-observer.changed:
		case <-clientDone:
			return observer.failure(codexsdk.ErrClientClosed)
		case <-ctx.Done():
			return observer.failure(ctx.Err())
		}
	}
}

// Only the synthetic fixture is retained; never walk the workspace or Codex home.
func captureEditDiagnostic(diagnostics *Diagnostics, fixture editFixture, phase string) {
	if diagnostics == nil {
		return
	}
	data, err := readEditFixture(fixture.path)
	if err != nil {
		diagnostics.Record("edit_file", map[string]any{"phase": phase, "status": "unavailable", "reason": err.Error()})
		return
	}
	artifact := diagnostics.Artifact("edit-"+phase+".txt", data)
	diagnostics.Record("edit_file", map[string]any{"phase": phase, "artifact": artifact, "sha256": editDigest(data), "bytes": len(data), "matches_expected": string(data) == fixture.expected, "before": fixture.seed, "after": string(data)})
}
