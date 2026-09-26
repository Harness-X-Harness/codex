package live

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

var (
	structuredEditWireName = regexp.MustCompile(`^local__structured_edit__[0-9a-f]{12}$`)
	applyPatchWireName     = regexp.MustCompile(`^local__apply_patch__[0-9a-f]{12}$`)
	commandWireName        = regexp.MustCompile(`^local__(?:exec_command|write_stdin|shell)__[0-9a-f]{12}$`)
)

func isStructuredEditIdentity(name string) bool {
	return name == "structured_edit" || structuredEditWireName.MatchString(name)
}

func isApplyPatchIdentity(name string) bool {
	return name == "apply_patch" || applyPatchWireName.MatchString(name)
}

func isCommandIdentity(name string) bool {
	switch name {
	case "shell", "exec_command", "write_stdin", "exec":
		return true
	default:
		return commandWireName.MatchString(name)
	}
}

func TestGrokStructuredEditWireContract(t *testing.T) {
	h := startGrokLive(t, liveOptions{disableShell: true})
	ex := h.acceptedResponses(context.Background())
	tool, ok := advertisedStructuredEdit(ex.requestBody)
	if !ok {
		h.failStage("structured_edit_advertised", "last /responses does not advertise a closed structured_edit function")
	}
	if advertisesApplyPatch(ex.requestBody) {
		h.failStage("apply_patch_not_advertised", fmt.Sprintf("last /responses still advertises apply_patch beside structured_edit %s", tool.Name))
	}
}

func TestGrokStructuredEdit(t *testing.T) {
	h := startGrokLive(t, liveOptions{disableShell: true})
	path := filepath.Join(h.workspace, structuredEditFile)
	if err := os.WriteFile(path, []byte(structuredEditSeed), 0o644); err != nil {
		t.Fatalf("seed structured_edit fixture: %v", err)
	}
	wantHash := sha256Hex([]byte(structuredEditExpected))
	ctx := context.Background()
	h.requireGrokCatalog(ctx)

	first := h.runTurn(ctx, startTurnOpts{
		prompt:        "Replace the exact text GROK_STRUCTURED_EDIT_SEED_v1 with GROK_STRUCTURED_EDIT_REPLACED_v1 in structured_edit_fixture.txt by calling structured_edit exactly once. Do not call apply_patch. Do not use a shell, exec_command, Python, sed, or any other tool. After the file contains GROK_STRUCTURED_EDIT_REPLACED_v1, stop without editing again.",
		deadline:      3 * time.Minute,
		approvalNever: true,
		dangerFull:    true,
		disableShell:  true,
	})
	if first.Provider != grokProvider {
		h.failStage("thread_bound_to_grok", "Thread is not bound to the Grok Provider")
	}
	if !first.completed() {
		h.failStage("turn_completed", "structured_edit Turn did not complete")
	}

	var ev structuredEditEvidence
	if !waitDurable(rolloutSettle, func() bool {
		ev = collectStructuredEditEvidence(first.Items, h.home)
		return ev.StructuredEditCalls == 1 && ev.FileChangeCompleted == 1
	}) {
		ev = collectStructuredEditEvidence(first.Items, h.home)
	}
	if ev.StructuredEditCalls != 1 {
		h.failStage("structured_edit_exactly_once", fmt.Sprintf("structured_edit identities=%d file_change_completed=%d, expected exactly one structured_edit invocation", ev.StructuredEditCalls, ev.FileChangeCompleted))
	}
	if ev.ApplyPatchCalls != 0 {
		h.failStage("apply_patch_absent", fmt.Sprintf("apply_patch identities=%d, expected zero", ev.ApplyPatchCalls))
	}
	if ev.CommandCalls != 0 || hasCommandExecution(first.Items) {
		h.failStage("command_execution_absent", fmt.Sprintf("command identities=%d command_execution=%t, expected zero", ev.CommandCalls, hasCommandExecution(first.Items)))
	}
	if ev.FileChangeCompleted != 1 || ev.FileChangeDeclined != 0 {
		h.failStage("file_change_completed", fmt.Sprintf("file_change completed=%d declined=%d failed=%d, expected one completed FileChange", ev.FileChangeCompleted, ev.FileChangeDeclined, ev.FileChangeFailed))
	}
	got, gotHash, err := readFileSHA256(path)
	if err != nil || !bytesEqual(got, []byte(structuredEditExpected)) || gotHash != wantHash {
		h.failStage("workspace_file_verified", fmt.Sprintf("fixture hash=%s want=%s readable=%t, expected exact structured_edit bytes", gotHash, wantHash, err == nil))
	}

	second := h.runTurn(ctx, startTurnOpts{
		threadID: first.ThreadID,
		prompt:   "Confirm that structured_edit_fixture.txt already contains GROK_STRUCTURED_EDIT_REPLACED_v1. Do not call structured_edit, apply_patch, or any shell. Reply with a short confirmation.",
		deadline: 2 * time.Minute,
	})
	if second.ThreadID != first.ThreadID || !second.completed() {
		h.failStage("continuation_turn_completed", fmt.Sprintf("Turn 2 same_thread=%t status=%s deadline=%t, expected terminal completed on the same Thread", second.ThreadID == first.ThreadID, second.Status, second.DeadlineHit))
	}
	last := lastResponsesExchange(h.recorder.snapshot())
	if last == nil || !structuredEditHistoryReplayed(last.requestBody) {
		h.failStage("structured_edit_replayed_accepted", "last /responses does not replay structured_edit function_call and function_call_output")
	}
	if last.status < 200 || last.status > 299 {
		h.failStage("structured_edit_replayed_accepted", fmt.Sprintf("replayed structured_edit history returned HTTP %d (%s), expected 2xx", last.status, collapsedBackendError(string(last.responseBody))))
	}
	secondEv := collectStructuredEditEvidence(second.Items, "")
	if secondEv.StructuredEditCalls != 0 || secondEv.ApplyPatchCalls != 0 || secondEv.CommandCalls != 0 {
		h.failStage("continuation_did_not_repeat_edit", fmt.Sprintf("Turn 2 structured_edit=%d apply_patch=%d command=%d, expected no new edit or shell", secondEv.StructuredEditCalls, secondEv.ApplyPatchCalls, secondEv.CommandCalls))
	}
	got, gotHash, err = readFileSHA256(path)
	if err != nil || !bytesEqual(got, []byte(structuredEditExpected)) || gotHash != wantHash {
		h.failStage("workspace_file_verified", "continuation changed the fixture after the structured edit")
	}
}

func TestGrokStructuredEditApprovalDeclined(t *testing.T) {
	h := startGrokLive(t, liveOptions{disableShell: true})
	path := filepath.Join(h.workspace, structuredEditFile)
	seed := []byte(structuredEditSeed)
	if err := os.WriteFile(path, seed, 0o644); err != nil {
		t.Fatalf("seed structured_edit fixture: %v", err)
	}
	wantHash := sha256Hex(seed)
	ctx := context.Background()
	h.requireGrokCatalog(ctx)

	run := h.runTurn(ctx, startTurnOpts{
		prompt:          "Replace the exact text GROK_STRUCTURED_EDIT_SEED_v1 with GROK_STRUCTURED_EDIT_REPLACED_v1 in structured_edit_fixture.txt by calling structured_edit exactly once. Do not call apply_patch. Do not use a shell, exec_command, Python, or sed. If the edit is declined, stop without retrying.",
		deadline:        3 * time.Minute,
		dangerFull:      true,
		disableShell:    true,
		allowFailedTurn: true,
	})
	if run.Provider != grokProvider {
		h.failStage("thread_bound_to_grok", "Thread is not bound to the Grok Provider")
	}

	var ev structuredEditEvidence
	if !waitDurable(rolloutSettle, func() bool {
		ev = collectStructuredEditEvidence(run.Items, h.home)
		return ev.StructuredEditCalls == 1 && ev.FileChangeDeclined >= 1
	}) {
		ev = collectStructuredEditEvidence(run.Items, h.home)
	}
	if ev.StructuredEditCalls != 1 {
		h.failStage("structured_edit_attempted", fmt.Sprintf("structured_edit identities=%d, expected exactly one declined attempt", ev.StructuredEditCalls))
	}
	if ev.FileChangeDeclined < 1 || ev.FileChangeCompleted != 0 {
		h.failStage("edit_declined", fmt.Sprintf("file_change completed=%d declined=%d failed=%d, expected a declined FileChange and no completed edit", ev.FileChangeCompleted, ev.FileChangeDeclined, ev.FileChangeFailed))
	}
	if ev.ApplyPatchCalls != 0 {
		h.failStage("apply_patch_absent", fmt.Sprintf("apply_patch identities=%d, expected zero", ev.ApplyPatchCalls))
	}
	if ev.CommandCalls != 0 || hasCommandExecution(run.Items) {
		h.failStage("command_execution_absent", fmt.Sprintf("command identities=%d command_execution=%t, expected zero", ev.CommandCalls, hasCommandExecution(run.Items)))
	}
	got, gotHash, err := readFileSHA256(path)
	if err != nil || !bytesEqual(got, seed) || gotHash != wantHash {
		h.failStage("workspace_file_unchanged", fmt.Sprintf("fixture hash=%s want=%s readable=%t, expected the declined edit to leave the seed bytes unchanged", gotHash, wantHash, err == nil))
	}
}

type grokAdvertisedTool struct {
	Type        string          `json:"type"`
	Name        string          `json:"name"`
	Parameters  json.RawMessage `json:"parameters"`
	Format      json.RawMessage `json:"format"`
	Description string          `json:"description"`
}

type structuredEditEvidence struct {
	StructuredEditCalls int
	ApplyPatchCalls     int
	CommandCalls        int
	FileChangeCompleted int
	FileChangeDeclined  int
	FileChangeFailed    int
}

func advertisedStructuredEdit(body []byte) (grokAdvertisedTool, bool) {
	for _, tool := range parseAdvertisedTools(body) {
		if tool.Type != "function" || !isStructuredEditIdentity(tool.Name) {
			continue
		}
		if !closedStructuredEditSchema(tool.Parameters) {
			return grokAdvertisedTool{}, false
		}
		return tool, true
	}
	return grokAdvertisedTool{}, false
}

func advertisesApplyPatch(body []byte) bool {
	for _, tool := range parseAdvertisedTools(body) {
		if isApplyPatchIdentity(tool.Name) {
			return true
		}
	}
	return false
}

func parseAdvertisedTools(body []byte) []grokAdvertisedTool {
	var parsed struct {
		Tools []grokAdvertisedTool `json:"tools"`
	}
	if json.Unmarshal(body, &parsed) != nil {
		return nil
	}
	return parsed.Tools
}

func closedStructuredEditSchema(raw json.RawMessage) bool {
	var schema struct {
		Type                 string                     `json:"type"`
		Properties           map[string]json.RawMessage `json:"properties"`
		Required             []string                   `json:"required"`
		AdditionalProperties *bool                      `json:"additionalProperties"`
	}
	if json.Unmarshal(raw, &schema) != nil {
		return false
	}
	if schema.Type != "object" || schema.AdditionalProperties == nil || *schema.AdditionalProperties {
		return false
	}
	required := []string{"file_path", "old_string", "new_string"}
	if len(schema.Required) != len(required) {
		return false
	}
	for _, key := range required {
		if !slices.Contains(schema.Required, key) {
			return false
		}
		if _, ok := schema.Properties[key]; !ok {
			return false
		}
	}
	if _, ok := schema.Properties["replace_all"]; !ok {
		return false
	}
	_, hasEnv := schema.Properties["environment_id"]
	return !hasEnv && !slices.Contains(schema.Required, "replace_all")
}

func structuredEditHistoryReplayed(body []byte) bool {
	var parsed struct {
		Input []struct {
			Type   string `json:"type"`
			Name   string `json:"name"`
			CallID string `json:"call_id"`
		} `json:"input"`
	}
	if json.Unmarshal(body, &parsed) != nil {
		return false
	}
	outputs := map[string]bool{}
	for _, item := range parsed.Input {
		if item.Type == "function_call_output" && item.CallID != "" {
			outputs[item.CallID] = true
		}
	}
	for _, item := range parsed.Input {
		if item.Type != "function_call" || !isStructuredEditIdentity(item.Name) || item.CallID == "" {
			continue
		}
		if outputs[item.CallID] {
			return true
		}
	}
	return false
}

func collectStructuredEditEvidence(items []protocolv2.ThreadItem, home string) structuredEditEvidence {
	var ev structuredEditEvidence
	for _, item := range items {
		if change, ok := item.AsFileChange(); ok {
			switch change.Status {
			case protocolv2.PatchApplyStatusCompleted:
				ev.FileChangeCompleted++
			case protocolv2.PatchApplyStatusDeclined:
				ev.FileChangeDeclined++
			case protocolv2.PatchApplyStatusFailed:
				ev.FileChangeFailed++
			}
			continue
		}
		if _, ok := item.AsCommandExecution(); ok {
			ev.CommandCalls++
		}
	}
	if home != "" {
		for _, item := range scanDurableResponseItems(home) {
			if item.Type != "function_call" && item.Type != "custom_tool_call" {
				continue
			}
			countToolIdentity(&ev, item.Name)
		}
		for _, status := range scanDurableFileChanges(home) {
			switch strings.ToLower(status) {
			case "completed":
				if ev.FileChangeCompleted == 0 {
					ev.FileChangeCompleted++
				}
			case "declined":
				if ev.FileChangeDeclined == 0 {
					ev.FileChangeDeclined++
				}
			case "failed":
				if ev.FileChangeFailed == 0 {
					ev.FileChangeFailed++
				}
			}
		}
		return ev
	}
	for _, item := range items {
		call, ok := item.AsDynamicToolCall()
		if !ok {
			continue
		}
		countToolIdentity(&ev, call.Tool)
	}
	return ev
}

func countToolIdentity(ev *structuredEditEvidence, name string) {
	switch {
	case isStructuredEditIdentity(name):
		ev.StructuredEditCalls++
	case isApplyPatchIdentity(name):
		ev.ApplyPatchCalls++
	case isCommandIdentity(name):
		ev.CommandCalls++
	}
}

func scanDurableFileChanges(home string) []string {
	var statuses []string
	_ = filepath.WalkDir(filepath.Join(home, "sessions"), func(path string, entry fs.DirEntry, err error) error {
		if err != nil || entry.IsDir() || !strings.HasSuffix(path, ".jsonl") {
			return err
		}
		data, readErr := os.ReadFile(path)
		if readErr != nil {
			return readErr
		}
		for _, raw := range bytes.Split(data, []byte("\n")) {
			raw = bytes.TrimSpace(raw)
			if len(raw) == 0 {
				continue
			}
			var rec struct {
				Type    string          `json:"type"`
				Payload json.RawMessage `json:"payload"`
			}
			if json.Unmarshal(raw, &rec) != nil || rec.Type != "event_msg" {
				continue
			}
			var event struct {
				Type string          `json:"type"`
				Item json.RawMessage `json:"item"`
			}
			if json.Unmarshal(rec.Payload, &event) != nil || event.Type != "item_completed" {
				continue
			}
			var completed struct {
				Type   string `json:"type"`
				Kind   string `json:"kind"`
				Status string `json:"status"`
			}
			if json.Unmarshal(event.Item, &completed) != nil {
				continue
			}
			kind := completed.Kind
			if kind == "" {
				kind = completed.Type
			}
			if kind != "FileChange" && kind != "fileChange" {
				continue
			}
			statuses = append(statuses, completed.Status)
		}
		return nil
	})
	return statuses
}

func bytesEqual(got, want []byte) bool {
	return string(got) == string(want)
}

func sha256Hex(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

func readFileSHA256(path string) ([]byte, string, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, "", err
	}
	return data, sha256Hex(data), nil
}

func TestStructuredEditIdentityRejectsContainsMatches(t *testing.T) {
	if !isStructuredEditIdentity("structured_edit") || !isStructuredEditIdentity("local__structured_edit__deadbeefcafe") {
		t.Fatal("canonical and exact wire identities must match")
	}
	if isStructuredEditIdentity("local__structured_edit__deadbeefcafeX") || isStructuredEditIdentity("not_structured_edit") || isStructuredEditIdentity("apply_patch") {
		t.Fatal("near-miss or apply_patch names must not count as structured_edit")
	}
	if !isApplyPatchIdentity("apply_patch") || !isApplyPatchIdentity("local__apply_patch__deadbeefcafe") {
		t.Fatal("canonical and exact apply_patch identities must match")
	}
	if isApplyPatchIdentity("local__structured_edit__deadbeefcafe") || isApplyPatchIdentity("custom_apply_patch") {
		t.Fatal("structured_edit or contains-style names must not count as apply_patch")
	}
	if !isCommandIdentity("exec_command") || !isCommandIdentity("local__write_stdin__deadbeefcafe") {
		t.Fatal("exact command identities must match")
	}
	if isCommandIdentity("structured_edit") || isCommandIdentity("local__structured_edit__deadbeefcafe") {
		t.Fatal("structured_edit must not count as a command identity")
	}
}

func TestAdvertisedStructuredEditRequiresClosedSchema(t *testing.T) {
	valid := []byte(`{"tools":[{"type":"function","name":"local__structured_edit__deadbeefcafe","parameters":{"type":"object","properties":{"file_path":{"type":"string"},"old_string":{"type":"string"},"new_string":{"type":"string"},"replace_all":{"type":"boolean"}},"required":["file_path","old_string","new_string"],"additionalProperties":false}}]}`)
	tool, ok := advertisedStructuredEdit(valid)
	if !ok || tool.Name != "local__structured_edit__deadbeefcafe" {
		t.Fatal("closed structured_edit function should be accepted")
	}
	if advertisesApplyPatch(valid) {
		t.Fatal("structured_edit advertisement must not count as apply_patch")
	}
	open := []byte(`{"tools":[{"type":"function","name":"local__structured_edit__deadbeefcafe","parameters":{"type":"object","properties":{"file_path":{},"old_string":{},"new_string":{},"replace_all":{}},"required":["file_path","old_string","new_string"]}}]}`)
	if _, ok := advertisedStructuredEdit(open); ok {
		t.Fatal("missing additionalProperties must not count as a closed schema")
	}
	custom := []byte(`{"tools":[{"type":"custom","name":"apply_patch","format":{"type":"grammar"}}]}`)
	if !advertisesApplyPatch(custom) {
		t.Fatal("custom apply_patch must be detected by exact identity")
	}
}

func TestStructuredEditHistoryReplayRequiresPairedOutput(t *testing.T) {
	body := []byte(`{"input":[{"type":"function_call","name":"local__structured_edit__deadbeefcafe","call_id":"c1"},{"type":"function_call_output","call_id":"c1"}]}`)
	if !structuredEditHistoryReplayed(body) {
		t.Fatal("paired structured_edit function_call/output should replay")
	}
	if structuredEditHistoryReplayed([]byte(`{"input":[{"type":"function_call","name":"local__structured_edit__deadbeefcafe","call_id":"c1"}]}`)) {
		t.Fatal("function_call without output must not count as replay")
	}
	if structuredEditHistoryReplayed([]byte(`{"input":[{"type":"custom_tool_call","name":"apply_patch","call_id":"c1"},{"type":"custom_tool_call_output","call_id":"c1"}]}`)) {
		t.Fatal("apply_patch custom history must not count as structured_edit replay")
	}
}

func TestCollectStructuredEditEvidenceCountsExactIdentities(t *testing.T) {
	items := []protocolv2.ThreadItem{
		protocolv2.NewThreadItemFileChange(protocolv2.ThreadItemFileChange{
			ID: "1", Status: protocolv2.PatchApplyStatusCompleted, Changes: []protocolv2.FileUpdateChange{},
		}),
	}
	home := t.TempDir()
	dir := filepath.Join(home, "sessions")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	line := `{"type":"response_item","payload":{"type":"function_call","name":"structured_edit"}}` + "\n"
	if err := os.WriteFile(filepath.Join(dir, "rollout.jsonl"), []byte(line), 0o600); err != nil {
		t.Fatal(err)
	}
	ev := collectStructuredEditEvidence(items, home)
	if ev.StructuredEditCalls != 1 || ev.FileChangeCompleted != 1 || ev.ApplyPatchCalls != 0 {
		t.Fatalf("evidence = %+v, expected one structured_edit identity and one completed FileChange", ev)
	}
}

func TestShippedGrokCatalogAdvertisesStructuredEdit(t *testing.T) {
	for _, model := range readShippedCatalog(t) {
		if model.ApplyPatchToolType != nil {
			t.Fatalf("%s apply_patch_tool_type = %q, want null", model.Slug, *model.ApplyPatchToolType)
		}
		if model.StructuredEditToolType == nil || *model.StructuredEditToolType != "exact_match" {
			got := "<nil>"
			if model.StructuredEditToolType != nil {
				got = *model.StructuredEditToolType
			}
			t.Fatalf("%s structured_edit_tool_type = %s, want exact_match", model.Slug, got)
		}
	}
}
