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
)

const (
	editFile = "structured_edit_fixture.txt"
	editOld  = "GROK_STRUCTURED_EDIT_SEED_v1"
	editNew  = "GROK_STRUCTURED_EDIT_REPLACED_v1"
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
	TurnCompleted                                bool
	Calls, PairedOutputs, FileChanges, Approvals int
	BeforeSHA256, AfterSHA256                    string
	BytesMatch, ContinuationUnchanged            bool
}

// StructuredEdit observes one exact edit and one same-thread continuation.
func StructuredEdit(ctx context.Context, options Options) (StructuredEditEvidence, error) {
	return runStructuredEdit(ctx, options, editFixture{continuation: true})
}

// StructuredEditApprovalDeclined declines the only file-change approval.
func StructuredEditApprovalDeclined(ctx context.Context, options Options) (StructuredEditEvidence, error) {
	return runStructuredEdit(ctx, options, editFixture{decline: true})
}

// StructuredEditPinnedPreviousModel exercises the pinned harness model once.
func StructuredEditPinnedPreviousModel(ctx context.Context, options Options) (StructuredEditEvidence, error) {
	options.Model = providerfixture.PinnedModel
	return runStructuredEdit(ctx, options, editFixture{})
}

// StructuredEditReplaceAll requires one call with the JSON boolean true.
func StructuredEditReplaceAll(ctx context.Context, options Options) (StructuredEditEvidence, error) {
	return runStructuredEdit(ctx, options, editFixture{replaceAll: true})
}

func runStructuredEdit(ctx context.Context, options Options, fixture editFixture) (StructuredEditEvidence, error) {
	fixture.seed = "prefix\r\n" + editOld + "\r\nsuffix"
	if fixture.replaceAll {
		fixture.seed += "\r\n" + editOld
	}
	fixture.expected = strings.ReplaceAll(fixture.seed, editOld, editNew)
	if fixture.decline {
		fixture.expected = fixture.seed
	}
	evidence := StructuredEditEvidence{BeforeSHA256: editDigest([]byte(fixture.seed))}
	policy := "never"
	if fixture.decline {
		policy = "untrusted"
	}
	base, err := runFixture(ctx, options, &fixture, map[string]any{
		"experimentalRawEvents": true, "approvalPolicy": policy,
	}, func(server *appServer, threadID string, base *Evidence) error {
		probe := editProbe{server: server, threadID: threadID, fixture: &fixture, evidence: &evidence, base: base}
		prompt := fmt.Sprintf("Call structured_edit exactly once to replace the exact text %s with %s in %s. Do not use any other tool. If declined, stop without retrying.", editOld, editNew, editFile)
		if fixture.replaceAll {
			prompt += " Set replace_all to the JSON boolean true and replace both occurrences."
		}
		if err := probe.observe(prompt); err != nil {
			return err
		}
		base.Stage = "edit_observed"
		if fixture.continuation {
			probe.previousID, probe.turnID = probe.turnID, ""
			if err := probe.observe("Confirm the previous edit is complete. Do not call any tool or edit the file again."); err != nil {
				return err
			}
			evidence.ContinuationUnchanged = true
			base.Stage = "edit_continuation_observed"
		}
		base.Completed = true
		return nil
	})
	evidence.Evidence = base
	return evidence, err
}

type editProbe struct {
	server                                   *appServer
	threadID, turnID, previousID, approvalID string
	fixture                                  *editFixture
	evidence                                 *StructuredEditEvidence
	base                                     *Evidence
}

type editItem struct {
	ID        string          `json:"id"`
	Type      string          `json:"type"`
	Name      string          `json:"name"`
	Namespace *string         `json:"namespace"`
	CallID    string          `json:"call_id"`
	Arguments string          `json:"arguments"`
	Output    json.RawMessage `json:"output"`
	Status    string          `json:"status"`
	Phase     string          `json:"phase"`
	Text      string          `json:"text"`
	Changes   []struct {
		Path string `json:"path"`
		Kind struct {
			Type     string  `json:"type"`
			MovePath *string `json:"move_path"`
		} `json:"kind"`
	} `json:"changes"`
}

type editTurn struct {
	ID     string          `json:"id"`
	Status string          `json:"status"`
	Error  json.RawMessage `json:"error"`
	Items  []editItem      `json:"items"`
}

type editObservation struct {
	callID, outputID, changeID, changeStatus string
	changeDigest                             [sha256.Size]byte
	completed                                bool
	replyBytes                               int
}

func (probe *editProbe) approval(message frame) error {
	var request struct {
		ThreadID string `json:"threadId"`
		TurnID   string `json:"turnId"`
		ItemID   string `json:"itemId"`
	}
	if !probe.fixture.decline || probe.previousID != "" || probe.approvalID != "" ||
		message.Method != "item/fileChange/requestApproval" || json.Unmarshal(message.Params, &request) != nil ||
		request.ThreadID != probe.threadID || request.TurnID == "" || request.ItemID == "" ||
		(probe.turnID != "" && request.TurnID != probe.turnID) {
		return probe.server.refuse(message.ID)
	}
	// Reconcile early approvals against turn/start and the exact call below.
	probe.turnID, probe.approvalID = request.TurnID, request.ItemID
	probe.evidence.Approvals++
	return probe.server.send(frame{ID: message.ID, Result: json.RawMessage(`{"decision":"decline"}`)})
}

func (probe *editProbe) observe(prompt string) error {
	probe.server.request = probe.approval
	defer func() { probe.server.request = nil }()
	probe.base.Turns++
	probe.base.Completed = false
	probe.evidence.TurnCompleted, probe.evidence.BytesMatch = false, false
	probe.evidence.AfterSHA256, probe.base.ReplyBytes = "", 0
	var started struct {
		Turn editTurn `json:"turn"`
	}
	if err := probe.server.call("turn/start", map[string]any{
		"threadId": probe.threadID,
		"input":    []any{map[string]any{"type": "text", "text": prompt, "textElements": []any{}}},
	}, &started); err != nil {
		return err
	}
	if started.Turn.ID == "" || started.Turn.ID == probe.previousID ||
		(probe.turnID != "" && started.Turn.ID != probe.turnID) {
		return errors.New("live: edit turn identity not established")
	}
	probe.turnID, probe.base.Stage = started.Turn.ID, "edit_turn_submitted"
	if probe.previousID != "" {
		probe.base.Stage = "edit_continuation_submitted"
	}
	observation := editObservation{}
	defer func() {
		probe.base.ReplyBytes = observation.replyBytes
		if probe.previousID == "" {
			probe.base.FirstReplyBytes = observation.replyBytes
		}
	}()
	if started.Turn.Status != "inProgress" {
		if err := probe.terminal(started.Turn, &observation); err != nil {
			return err
		}
	}
	for frames := 0; !observation.completed; frames++ {
		if frames == 4096 {
			return errors.New("live: edit evidence budget exceeded")
		}
		message, err := probe.server.next()
		if err != nil {
			return err
		}
		if err := probe.notification(message, &observation); err != nil {
			return err
		}
	}
	// Read the settled thread once. This also fences and drains notifications
	// queued after terminal completion; it never starts or resubmits a turn.
	var read struct {
		Thread struct {
			ID    string     `json:"id"`
			Turns []editTurn `json:"turns"`
		} `json:"thread"`
	}
	if err := probe.server.call("thread/read", map[string]any{"threadId": probe.threadID, "includeTurns": true}, &read); err != nil {
		return err
	}
	for len(probe.server.pending) != 0 {
		message, err := probe.server.next()
		if err != nil {
			return err
		}
		if err := probe.notification(message, &observation); err != nil {
			return err
		}
	}
	if read.Thread.ID != probe.threadID || len(read.Thread.Turns) != probe.base.Turns ||
		read.Thread.Turns[len(read.Thread.Turns)-1].ID != probe.turnID {
		return errors.New("live: settled edit thread identity not established")
	}
	if err := probe.terminal(read.Thread.Turns[len(read.Thread.Turns)-1], &observation); err != nil {
		return err
	}
	if probe.previousID == "" {
		status := "completed"
		if probe.fixture.decline {
			status = "declined"
		}
		if observation.callID == "" || observation.outputID != observation.callID ||
			observation.changeID != observation.callID || observation.changeStatus != status ||
			(probe.fixture.decline && probe.approvalID != observation.callID) {
			// Classify the first missing correlation without exposing IDs or tool
			// content. The success predicate and proof latches above stay unchanged.
			code := "edit_approval_unpaired"
			switch {
			case observation.callID == "":
				code = "edit_call_missing"
			case observation.outputID == "":
				code = "edit_output_missing"
			case observation.outputID != observation.callID:
				code = "edit_output_unpaired"
			case observation.changeID == "":
				code = "edit_change_missing"
			case observation.changeID != observation.callID:
				code = "edit_change_unpaired"
			case observation.changeStatus != status:
				code = "edit_change_status_mismatch"
				switch observation.changeStatus {
				case "failed":
					code = "edit_change_failed"
				case "declined":
					code = "edit_change_declined"
				}
			}
			return failedObservation(code, "live: edit call, output or file-change evidence missing")
		}
		probe.evidence.Calls, probe.evidence.PairedOutputs, probe.evidence.FileChanges = 1, 1, 1
	}
	if observation.replyBytes == 0 {
		return failedObservation("edit_reply_missing", "live: edit final reply unavailable")
	}
	info, err := os.Lstat(probe.fixture.path)
	if err != nil || !info.Mode().IsRegular() || info.Size() > 4096 {
		return errors.New("live: edit fixture is not a bounded regular file")
	}
	file, err := os.Open(probe.fixture.path)
	if err != nil {
		return errors.New("live: edit fixture unavailable")
	}
	opened, statErr := file.Stat()
	if statErr != nil || !os.SameFile(info, opened) {
		_ = file.Close()
		return errors.New("live: edit fixture identity changed")
	}
	data, err := io.ReadAll(io.LimitReader(file, 4097))
	_ = file.Close()
	if err != nil || len(data) > 4096 {
		return errors.New("live: edit fixture unavailable or oversized")
	}
	probe.evidence.BytesMatch, probe.evidence.AfterSHA256 = false, editDigest(data)
	if string(data) != probe.fixture.expected {
		return failedObservation("edit_bytes_mismatch", "live: edit fixture bytes did not match")
	}
	probe.evidence.BytesMatch = true
	return nil
}

func (probe *editProbe) notification(message frame, observation *editObservation) error {
	if len(message.ID) != 0 {
		return probe.approval(message)
	}
	if message.Method != "turn/completed" && message.Method != "item/completed" && message.Method != "rawResponseItem/completed" {
		return nil
	}
	var event struct {
		ThreadID string   `json:"threadId"`
		TurnID   string   `json:"turnId"`
		Turn     editTurn `json:"turn"`
		Item     editItem `json:"item"`
	}
	if json.Unmarshal(message.Params, &event) != nil {
		return errors.New("live: invalid edit evidence")
	}
	if event.ThreadID != probe.threadID {
		return errors.New("live: unrelated edit thread evidence")
	}
	if message.Method == "turn/completed" {
		if event.Turn.ID != probe.turnID {
			return errors.New("live: unrelated edit turn evidence")
		}
		return probe.terminal(event.Turn, observation)
	}
	if event.TurnID != probe.turnID {
		return errors.New("live: unrelated edit turn evidence")
	}
	if message.Method == "rawResponseItem/completed" {
		return probe.rawItem(event.Item, observation)
	}
	return probe.publicItem(event.Item, observation)
}

func (probe *editProbe) terminal(turn editTurn, observation *editObservation) error {
	if turn.ID != probe.turnID || turn.Status != "completed" || len(turn.Error) != 0 && string(turn.Error) != "null" {
		return errors.New("live: edit turn did not complete successfully")
	}
	observation.completed = true
	probe.evidence.TurnCompleted = true
	probe.base.Stage = "edit_continuation_completed"
	if probe.previousID == "" {
		probe.base.FirstCompleted, probe.base.Stage = true, "edit_turn_completed"
	}
	for _, item := range turn.Items {
		if err := probe.publicItem(item, observation); err != nil {
			return err
		}
	}
	return nil
}

func (probe *editProbe) rawItem(item editItem, observation *editObservation) error {
	switch item.Type {
	case "function_call":
		if probe.previousID != "" || observation.callID != "" || !structuredEditIdentity(item.Name) || item.Namespace != nil || item.CallID == "" ||
			!editArgumentsMatch(item.Arguments, probe.fixture.path, probe.fixture.replaceAll) {
			return failedObservation("invalid_edit_invocation", "live: unexpected or invalid edit invocation")
		}
		observation.callID = item.CallID
	case "function_call_output":
		if probe.previousID != "" || observation.outputID != "" || item.CallID == "" ||
			(item.Name != "" && !structuredEditIdentity(item.Name)) || item.Namespace != nil || !editOutputPresent(item.Output) {
			return failedObservation("invalid_edit_output", "live: unexpected or invalid edit output")
		}
		observation.outputID = item.CallID
	case "custom_tool_call", "custom_tool_call_output", "local_shell_call", "tool_search_call":
		return failedObservation("alternative_tool_observed", "live: alternative tool evidence observed")
	}
	return nil
}

func (probe *editProbe) publicItem(item editItem, observation *editObservation) error {
	switch item.Type {
	case "agentMessage":
		if item.ID == "" {
			return errors.New("live: edit reply identity unavailable")
		}
		observation.replyBytes = 0
		if (item.Phase == "" || item.Phase == "final_answer") && strings.TrimSpace(item.Text) != "" {
			observation.replyBytes = len(item.Text)
		}
	case "fileChange":
		if probe.previousID != "" || item.ID == "" || len(item.Changes) != 1 ||
			filepath.Clean(item.Changes[0].Path) != probe.fixture.path || item.Changes[0].Kind.Type != "update" || item.Changes[0].Kind.MovePath != nil {
			return failedObservation("invalid_edit_file_change", "live: unexpected edit file change")
		}
		body, _ := json.Marshal(item.Changes)
		digest := sha256.Sum256(body)
		if observation.changeID != "" && (observation.changeID != item.ID || observation.changeStatus != item.Status || observation.changeDigest != digest) {
			return errors.New("live: conflicting or repeated edit file change")
		}
		observation.changeID, observation.changeStatus, observation.changeDigest = item.ID, item.Status, digest
	case "commandExecution", "dynamicToolCall", "mcpToolCall", "collabAgentToolCall":
		return failedObservation("alternative_tool_observed", "live: alternative tool evidence observed")
	}
	return nil
}

func editDigest(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}
