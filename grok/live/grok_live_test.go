package live

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func TestGrokBasic(t *testing.T) {
	h := startGrokLive(t, liveOptions{})
	ctx := context.Background()
	h.requireGrokCatalog(ctx)

	run := h.runTurn(ctx, startTurnOpts{
		prompt:   "Reply with a short confirmation that the Grok Turn completed.",
		deadline: 2 * time.Minute,
	})
	if run.Provider != grokProvider {
		h.failStage("thread_bound_to_grok", "Thread is not bound to the Grok Provider")
	}
	if run.Model != grokModel {
		h.failStage("thread_model_grok_4_6", "Thread model is not grok-4.6")
	}
	if !run.completed() {
		h.failStage("turn_completed", "ordinary Grok Turn did not complete")
	}
	if run.reply() == "" {
		h.failStage("agent_message_persisted", "Turn completed without an agent message")
	}
}

func TestGrokEncryptedReasoningContinuation(t *testing.T) {
	h := startGrokLive(t, liveOptions{probeTool: true})
	ctx := context.Background()
	h.requireGrokCatalog(ctx)

	first := h.runTurn(ctx, startTurnOpts{
		prompt:    "Use the " + probeToolName + " tool. After it returns, include its result in your reply and then stop.",
		deadline:  2 * time.Minute,
		probeTool: true,
	})
	if first.Provider != grokProvider {
		h.failStage("thread_bound_to_grok", "Thread is not bound to the Grok Provider")
	}
	if !first.completed() {
		h.failStage("first_turn_completed", "reasoning/tool Turn did not complete")
	}
	if h.requests.toolCalls < 1 && !hasCompletedProbe(first.Items) {
		h.failStage("named_tool_completed", "named dynamic tool did not complete")
	}
	if !durableContainsToken(h.home, probeToolOutput) && !strings.Contains(first.reply(), probeToolOutput) {
		if !waitDurable(rolloutSettle, func() bool { return durableContainsToken(h.home, probeToolOutput) }) {
			h.failStage("tool_result_in_history", "fresh tool result is missing from durable history")
		}
	}

	history := h.runTurn(ctx, startTurnOpts{
		threadID: first.ThreadID,
		prompt:   "Reply with exactly the result returned by " + probeToolName + " in the previous Turn. Do not call any tool.",
		deadline: 2 * time.Minute,
	})
	if history.ThreadID != first.ThreadID {
		h.failStage("same_thread_continuation", "continuation ran on another Thread")
	}
	if !history.completed() {
		h.failStage("history_turn_completed", "continuation Turn did not complete")
	}
	if !strings.Contains(history.reply(), probeToolOutput) {
		h.failStage("history_reply_contains_tool_result", "continuation reply does not contain the earlier tool result")
	}
	if !waitDurable(rolloutSettle, func() bool { return durableHasEncryptedReasoning(h.home) }) {
		h.failStage("encrypted_reasoning_observed", "durable Thread state has no encrypted reasoning")
	}
}

func TestGrokCollaboration(t *testing.T) {
	h := startGrokLive(t, liveOptions{})
	ctx := context.Background()
	h.requireGrokCatalog(ctx)

	run := h.runTurn(ctx, startTurnOpts{
		prompt:   "Delegate one bounded task to a child named live_child using the default full-history fork. Tell the child: Without running any commands or tools, write a fresh UUID v4 yourself and reply with exactly its canonical lowercase text and no other text. Wait for that child to complete, then reply with exactly the UUID returned by the child and no other text.",
		deadline: 6 * time.Minute,
		effort:   "ultra",
	})
	if run.Provider != grokProvider {
		h.failStage("parent_bound_to_grok", "parent Thread is not bound to the Grok Provider")
	}
	if !run.completed() {
		h.failStage("parent_turn_completed", "parent Turn did not complete")
	}
	parentNonce := nonceOf(run.reply())
	if !uuidShaped.MatchString(parentNonce) {
		h.failStage("parent_reply_has_fresh_nonce", "parent terminal reply is not a UUID-shaped child result")
	}

	includeTurns := true
	var child *protocolv2.Thread
	if !waitDurable(rolloutSettle, func() bool {
		listed, err := h.client.Threads().List(ctx, protocolv2.ThreadListParams{
			AncestorThreadID: protocolv2.Value(run.ThreadID),
		})
		if err != nil {
			return false
		}
		for i := range listed.Data {
			candidate := listed.Data[i]
			if candidate.ID == run.ThreadID {
				continue
			}
			parentID := ""
			if candidate.ParentThreadID != nil && candidate.ParentThreadID.Value != nil {
				parentID = *candidate.ParentThreadID.Value
			}
			forkedFrom := ""
			if candidate.ForkedFromID != nil && candidate.ForkedFromID.Value != nil {
				forkedFrom = *candidate.ForkedFromID.Value
			}
			if parentID == run.ThreadID || forkedFrom == run.ThreadID {
				if candidate.ModelProvider != grokProvider {
					return false
				}
				read, err := h.client.Threads().Read(ctx, protocolv2.ThreadReadParams{
					ThreadID:     candidate.ID,
					IncludeTurns: &includeTurns,
				})
				if err != nil {
					return false
				}
				child = &read.Thread
				return true
			}
		}
		return false
	}) || child == nil {
		h.failStage("child_linked_to_parent", "no Grok child Thread names the parent")
	}
	if child.ModelProvider != grokProvider {
		h.failStage("child_provider_verified", "child Thread is not bound to Grok")
	}
	childReply := ""
	for _, turn := range child.Turns {
		if text := lastAgentMessage(turn.Items); text != "" {
			childReply = text
		}
	}
	if childReply == "" {
		childReply = strings.TrimSpace(child.Preview)
	}
	if nonceOf(childReply) != parentNonce {
		h.failStage("parent_consumed_child_result", "parent terminal result does not match the child nonce")
	}
}

func TestGrokImageGenerationEdit(t *testing.T) {
	h := startGrokLive(t, liveOptions{})
	ctx := context.Background()
	h.requireGrokCatalog(ctx)

	generation := h.runTurn(ctx, startTurnOpts{
		prompt:   "Generate an image of a blue circle on a plain white background.",
		deadline: 3 * time.Minute,
	})
	if generation.Provider != grokProvider {
		h.failStage("thread_bound_to_grok", "Thread is not bound to the Grok Provider")
	}
	if !generation.completed() {
		h.failStage("generation_turn_completed", "image generation Turn did not complete")
	}
	generatedPath, err := newestCompletedImage(h, "", nil)
	if err != nil {
		h.failStage("generation_image_completed", err.Error())
	}
	generatedBytes, err := os.ReadFile(generatedPath)
	if err != nil {
		h.failStage("generation_image_verified", "generated image is no longer user-accessible")
	}

	edit := h.runTurn(ctx, startTurnOpts{
		threadID: generation.ThreadID,
		prompt:   "Edit the image you just generated so the circle is green while keeping the plain white background.",
		deadline: 3 * time.Minute,
	})
	if edit.ThreadID != generation.ThreadID {
		h.failStage("same_thread_edit", "edit Turn ran on another Thread")
	}
	if !edit.completed() {
		h.failStage("edit_turn_completed", "image edit Turn did not complete")
	}
	editedPath, err := newestCompletedImage(h, generatedPath, generatedBytes)
	if err != nil {
		h.failStage("edit_image_completed", err.Error())
	}
	editedBytes, err := os.ReadFile(editedPath)
	if err != nil {
		h.failStage("edit_image_verified", "edited image is no longer user-accessible")
	}
	if string(generatedBytes) == string(editedBytes) {
		h.failStage("edit_artifact_distinct", "edited image is byte-identical to the generated image")
	}
	if !imageViewsPath(edit.Items, generatedPath) && !scanDurableFacts(h.home).historyImageRef && !durableContainsToken(h.home, filepath.Base(generatedPath)) {
		h.failStage("edit_uses_history_image", "edit Turn does not show use of the generated image from Thread history")
	}
}

var errNoCompletedImage = errors.New("no completed image result is available to the user")

func newestCompletedImage(h *liveHarness, previousPath string, previousBytes []byte) (string, error) {
	var chosen durableImage
	if !waitDurable(rolloutSettle, func() bool {
		for _, image := range scanDurableFacts(h.home).images {
			if image.Status != "completed" || image.SavedPath == "" || image.Result == "" {
				continue
			}
			if previousPath != "" {
				payload, err := decodeImagePayload(image.Result)
				if err != nil {
					continue
				}
				if image.SavedPath == previousPath && string(payload) == string(previousBytes) {
					continue
				}
			}
			chosen = image
			return true
		}
		return false
	}) {
		return "", fmt.Errorf("%w (%s)", errNoCompletedImage, durableTypeInventory(h.home))
	}
	item := protocolv2.ThreadItemImageGeneration{
		Result:    chosen.Result,
		SavedPath: protocolv2.Value(chosen.SavedPath),
		Status:    chosen.Status,
	}
	path, _, err := verifySavedImage(item)
	return path, err
}

func TestGrokCustomApplyPatch(t *testing.T) {
	h := startGrokLive(t, liveOptions{disableShell: true})
	if err := os.WriteFile(filepath.Join(h.workspace, applyPatchFile), []byte(applyPatchSeed), 0o644); err != nil {
		t.Fatalf("seed workspace file: %v", err)
	}
	ctx := context.Background()
	h.requireGrokCatalog(ctx)

	run := h.runTurn(ctx, startTurnOpts{
		prompt:        "Replace the exact contents of hello.txt from HELLO to WORLD using apply_patch. Do not use a shell or exec_command. When the file contains WORLD, stop.",
		deadline:      2 * time.Minute,
		approvalNever: true,
		dangerFull:    true,
		disableShell:  true,
	})
	if run.Provider != grokProvider {
		h.failStage("thread_bound_to_grok", "Thread is not bound to the Grok Provider")
	}
	if !run.completed() {
		h.failStage("turn_completed", "apply_patch Turn did not complete")
	}
	var facts durableFacts
	waitDurable(rolloutSettle, func() bool {
		facts = scanDurableFacts(h.home)
		return applyPatchObserved(run.Items) || facts.applyPatchCall || facts.fileChange
	})
	if !applyPatchObserved(run.Items) && !facts.applyPatchCall && !facts.fileChange {
		h.failStage("custom_apply_patch_path", "custom apply_patch or equivalent file_change is missing")
	}
	if hasCommandExecution(run.Items) || facts.commandExecution {
		h.failStage("shell_edit_absent", "a shell path explained the edit")
	}
	contents, err := os.ReadFile(filepath.Join(h.workspace, applyPatchFile))
	if err != nil || strings.TrimSpace(string(contents)) != applyPatchExpected {
		h.failStage("workspace_file_verified", "workspace file does not show the expected result")
	}
}
