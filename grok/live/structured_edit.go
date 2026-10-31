package live

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"os"
	"strings"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
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
	return sdkStructuredEdit(ctx, options, fixture)
}

type editProbe struct {
	threadID, turnID, previousID string
	fixture                      *editFixture
	evidence                     *StructuredEditEvidence
	base                         *Evidence
}

type editItem struct {
	ID     string `json:"id"`
	Type   string `json:"type"`
	Status string `json:"status"`
	Phase  string `json:"phase"`
	Text   string `json:"text"`
}

type editTurn struct {
	ID     string          `json:"id"`
	Status string          `json:"status"`
	Error  json.RawMessage `json:"error"`
	Items  []editItem      `json:"items"`
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

func (probe *editProbe) publicItem(item editItem, observation *editObservation) error {
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
