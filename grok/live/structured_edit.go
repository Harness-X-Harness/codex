package live

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
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
	return sdkStructuredEdit(ctx, options, fixture)
}

type editProbe struct {
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

func (probe *editProbe) terminal(turn editTurn, observation *editObservation) error {
	if turn.ID != probe.turnID {
		return errors.New("live: edit turn did not complete successfully")
	}
	if turn.Status != "completed" || len(turn.Error) != 0 && string(turn.Error) != "null" {
		return failedTurn(turn.Status, turn.Error, "live: edit turn did not complete successfully")
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
			return failedObservation("invalid_edit_evidence", "live: edit reply identity unavailable")
		}
		observation.replyBytes = 0
		if (item.Phase == "" || item.Phase == "final_answer") && strings.TrimSpace(item.Text) != "" {
			observation.replyBytes = len(item.Text)
		}
	case "fileChange":
		if probe.previousID != "" || item.ID == "" || len(item.Changes) != 1 ||
			filepath.Base(item.Changes[0].Path) != editFile || item.Changes[0].Kind.Type != "update" || item.Changes[0].Kind.MovePath != nil {
			return failedObservation("invalid_edit_file_change", "live: unexpected edit file change")
		}
		body, _ := json.Marshal(item.Changes)
		digest := sha256.Sum256(body)
		if observation.changeID != "" && (observation.changeID != item.ID || observation.changeStatus != item.Status || observation.changeDigest != digest) {
			return failedObservation("conflicting_edit_item", "live: conflicting or repeated edit file change")
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
