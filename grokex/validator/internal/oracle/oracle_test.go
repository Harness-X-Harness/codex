package oracle

import (
	"bytes"
	"encoding/base64"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"reflect"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grokex/validator/internal/driver"
	"github.com/Harness-X-Harness/codex/grokex/validator/internal/rollout"
)

const (
	imageRoot    = "01a062cb-2f05-74b3-981e-7a01ab0c5fc1"
	imageTurnGen = "01a062cb-2f59-7b03-87a2-8a932b79e846"
	imageTurnEd  = "01a062cb-316c-70e3-887a-b61b4d9bbe42"
	collabRoot   = "01a062cd-189e-7b21-bb5c-9988de260c79"
	collabRootT  = "01a062cd-18bb-71f3-9b11-224bd51b1a30"
	nonce        = "3f2b1c9e-8d4a-4b6f-9a1e-7c5d2e8f0a11"
)

func fixtures(t *testing.T, name string) *rollout.Graph {
	t.Helper()
	graph, err := rollout.Scan(filepath.Join("..", "rollout", "testdata", name))
	if err != nil {
		t.Fatal(err)
	}
	return graph
}

func completedRun(thread, turn, reply string, seconds float64) driver.TurnRun {
	return driver.TurnRun{
		ThreadID:      thread,
		TurnID:        turn,
		Status:        "completed",
		FinalResponse: reply,
		Duration:      time.Duration(seconds * float64(time.Second)),
	}
}

// materialize writes each fixture image result to a fresh file so the saved
// path exists; the edit result becomes a distinct PNG to exercise codec checks.
func materialize(t *testing.T, graph *rollout.Graph) {
	t.Helper()
	dir := t.TempDir()
	root, _ := graph.Session(imageRoot)
	generation, _ := root.Turn(imageTurnGen)
	edit, _ := root.Turn(imageTurnEd)

	payload, err := base64.StdEncoding.DecodeString(generation.ImageResults[0].Result)
	if err != nil {
		t.Fatal(err)
	}
	generation.ImageResults[0].SavedPath = filepath.Join(dir, "generate-1.jpg")
	if err := os.WriteFile(generation.ImageResults[0].SavedPath, payload, 0o644); err != nil {
		t.Fatal(err)
	}

	var buffer bytes.Buffer
	canvas := image.NewNRGBA(image.Rect(0, 0, 2, 2))
	canvas.Set(0, 0, color.NRGBA{G: 255, A: 255})
	if err := png.Encode(&buffer, canvas); err != nil {
		t.Fatal(err)
	}
	edit.ImageResults[0].Result = base64.StdEncoding.EncodeToString(buffer.Bytes())
	edit.ImageResults[0].SavedPath = filepath.Join(dir, "edit-1.png")
	if err := os.WriteFile(edit.ImageResults[0].SavedPath, buffer.Bytes(), 0o644); err != nil {
		t.Fatal(err)
	}
}

func TestImageProvesGenerationAndHistoryEdit(t *testing.T) {
	graph := fixtures(t, "image")
	materialize(t, graph)

	verdict := Image(graph,
		completedRun(imageRoot, imageTurnGen, "Generated the image.", 12.5),
		completedRun(imageRoot, imageTurnEd, "Edited the image.", 9),
	)

	if !verdict.OK() {
		t.Fatalf("verdict failed: %s (%s) at %s", verdict.Failure, verdict.FailureCategory, verdict.LastProvenStage)
	}
	want := map[string]any{
		"edit_agent_reply_seen":         true,
		"edit_artifact_distinct":        true,
		"edit_artifact_extension":       ".png",
		"edit_artifact_match":           true,
		"edit_completion":               "completed",
		"edit_image_decodable":          true,
		"edit_image_mime":               "image/png",
		"evidence_source":               "canonical_session",
		"generation_agent_reply_seen":   true,
		"generation_artifact_extension": ".jpg",
		"generation_artifact_match":     true,
		"generation_completion":         "completed",
		"generation_image_decodable":    true,
		"generation_image_mime":         "image/jpeg",
		"history_arguments_verified":    true,
		"runner_turn_submission_count":  2,
		"same_thread":                   true,
		"status":                        "completed",
	}
	if !reflect.DeepEqual(verdict.Assertions, want) {
		t.Fatalf("assertions = %#v", verdict.Assertions)
	}
	if verdict.Diagnostics["image_items_completed"] != 2 || verdict.LastProvenStage != "edit_artifact_distinct" {
		t.Fatalf("diagnostics = %#v stage=%s", verdict.Diagnostics, verdict.LastProvenStage)
	}
}

func TestImageRejectsEditWithoutHistoryReference(t *testing.T) {
	graph := fixtures(t, "image")
	materialize(t, graph)
	root, _ := graph.Session(imageRoot)
	edit, _ := root.Turn(imageTurnEd)
	edit.FunctionCalls[0].Arguments = `{"prompt":"make the circle green"}`

	verdict := Image(graph,
		completedRun(imageRoot, imageTurnGen, "Generated the image.", 1),
		completedRun(imageRoot, imageTurnEd, "Edited the image.", 1),
	)

	if verdict.OK() || verdict.FailureCategory != "semantic_contract" || verdict.LastProvenStage != "edit_artifact_verified" {
		t.Fatalf("verdict = %+v", verdict)
	}
	if _, ok := verdict.Assertions["history_arguments_verified"]; ok {
		t.Fatal("failed contract must not be asserted")
	}
}

func TestImageDeadlineIsClassifiedNotProven(t *testing.T) {
	graph := fixtures(t, "image")
	materialize(t, graph)
	expired := completedRun(imageRoot, imageTurnEd, "", 180)
	expired.Status = "inProgress"
	expired.DeadlineExpired = true

	verdict := Image(graph, completedRun(imageRoot, imageTurnGen, "Generated the image.", 1), expired)

	if verdict.OK() || verdict.FailureCategory != "deadline" || verdict.LastProvenStage != "same_thread" {
		t.Fatalf("verdict = %+v", verdict)
	}
	if got := verdict.Diagnostics["edit_function_calls"]; !reflect.DeepEqual(got, map[string]int{"image_gen.imagegen": 1}) {
		t.Fatalf("post-mortem hints missing: %#v", verdict.Diagnostics)
	}
}

func TestReferencesHistory(t *testing.T) {
	cases := map[string]bool{
		`{"prompt":"x","num_last_images_to_include":1}`:                                     true,
		`{"prompt":"x","num_last_images_to_include":0}`:                                     false,
		`{"prompt":"x","num_last_images_to_include":1,"referenced_image_paths":["/a.jpg"]}`: false,
		`{"prompt":"x","referenced_image_paths":["/tmp/gen/generate-1.jpg"]}`:               true,
		`{"prompt":"x","referenced_image_paths":["/tmp/other.jpg"]}`:                        false,
		`{"prompt":"x"}`: false,
		`not json`:       false,
	}
	for arguments, want := range cases {
		if got := referencesHistory(arguments, "/tmp/gen/generate-1.jpg"); got != want {
			t.Errorf("referencesHistory(%s) = %v, want %v", arguments, got, want)
		}
	}
}

func TestVerifyArtifactRejectsMismatchedFile(t *testing.T) {
	graph := fixtures(t, "image")
	materialize(t, graph)
	root, _ := graph.Session(imageRoot)
	generation, _ := root.Turn(imageTurnGen)
	result := generation.ImageResults[0]
	if err := os.WriteFile(result.SavedPath, []byte("not the payload"), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := VerifyArtifact(result); err == nil {
		t.Fatal("artifact differing from the persisted result must fail")
	}
}

func TestCollaborationProvesChildLifecycleAndDelivery(t *testing.T) {
	graph := fixtures(t, "collab")
	catalog := driver.Catalog{ModelListed: true, MultiAgentVersion: "v2"}

	verdict := Collaboration(graph, completedRun(collabRoot, collabRootT, nonce, 240), catalog)

	if !verdict.OK() {
		t.Fatalf("verdict failed: %s (%s) at %s", verdict.Failure, verdict.FailureCategory, verdict.LastProvenStage)
	}
	want := map[string]any{
		"child_completion":             "completed",
		"child_parent_link_verified":   true,
		"child_provider_binding":       "grok/grok-4.6",
		"child_provider_verified":      true,
		"child_response_assertion":     "canonical_uuid_v4",
		"default_full_history":         "completed",
		"evidence_source":              "canonical_session",
		"multi_agent_version":          "v2",
		"parent_completion":            "completed",
		"reasoning_effort":             "ultra",
		"response_assertion":           "child_echo_match",
		"result_delivery":              "completed",
		"result_delivery_verified":     true,
		"runner_turn_submission_count": 1,
		"status":                       "completed",
	}
	if !reflect.DeepEqual(verdict.Assertions, want) {
		t.Fatalf("assertions = %#v", verdict.Assertions)
	}
	if verdict.Diagnostics["child_count"] != 1 || !reflect.DeepEqual(verdict.Diagnostics["root_function_calls"], map[string]int{"collaboration.spawn_agent": 1, "collaboration.wait_agent": 1}) {
		t.Fatalf("diagnostics = %#v", verdict.Diagnostics)
	}
}

func TestCollaborationRequiresDeliveredReplyToMatchCanonicalReply(t *testing.T) {
	graph := fixtures(t, "collab")

	verdict := Collaboration(graph, completedRun(collabRoot, collabRootT, "something else", 1), driver.Catalog{})

	if verdict.OK() || verdict.FailureCategory != "delivery" || verdict.LastProvenStage != "root_echo_match" {
		t.Fatalf("verdict = %+v", verdict)
	}
}

func TestCollaborationDeadlineKeepsPostMortemHints(t *testing.T) {
	graph := fixtures(t, "collab")
	run := completedRun(collabRoot, collabRootT, "", 360)
	run.Status = "inProgress"
	run.DeadlineExpired = true

	verdict := Collaboration(graph, run, driver.Catalog{})

	if verdict.OK() || verdict.FailureCategory != "deadline" || verdict.LastProvenStage != "root_session_found" {
		t.Fatalf("verdict = %+v", verdict)
	}
	if verdict.Diagnostics["root_sub_agent_completions"] != 1 {
		t.Fatalf("hints = %#v", verdict.Diagnostics)
	}
}

const (
	probeRoot     = "01a065f9-eae0-7fc0-9b66-0d2b9cafbeda"
	probeTurnOne  = "01a065f9-eb03-7d33-9375-30a66bd2d535"
	probeTurnHist = "01a065f9-ece6-7591-82b1-d0583f69c80c"
)

func TestBasicProvesBindingReplyAndDelivery(t *testing.T) {
	graph := fixtures(t, "image")

	verdict := Basic(graph, completedRun(imageRoot, imageTurnGen, "Generated the image.", 3))

	if !verdict.OK() {
		t.Fatalf("verdict failed: %s (%s) at %s", verdict.Failure, verdict.FailureCategory, verdict.LastProvenStage)
	}
	want := map[string]any{
		"evidence_source":              "canonical_session",
		"provider_binding":             "grok/grok-4.6",
		"response_assertion":           "nonempty_agent_message",
		"result_delivery_verified":     true,
		"runner_turn_submission_count": 1,
		"status":                       "completed",
	}
	if !reflect.DeepEqual(verdict.Assertions, want) {
		t.Fatalf("assertions = %#v", verdict.Assertions)
	}
	if verdict.Diagnostics["reply_matches_requested_marker"] != false {
		t.Fatalf("marker hint = %#v", verdict.Diagnostics["reply_matches_requested_marker"])
	}
}

func TestBasicRequiresDeliveredReplyToMatch(t *testing.T) {
	graph := fixtures(t, "image")
	verdict := Basic(graph, completedRun(imageRoot, imageTurnGen, "something else", 3))
	if verdict.OK() || verdict.FailureCategory != "delivery" || verdict.LastProvenStage != "reply_persisted" {
		t.Fatalf("verdict = %+v", verdict)
	}
}

func TestContinuationProvesToolRoundTripEncryptedReasoningAndHistory(t *testing.T) {
	graph := fixtures(t, "probe")

	verdict := Continuation(graph,
		completedRun(probeRoot, probeTurnOne, "GROKEX_LIVE_RESPONSE_OK", 20),
		completedRun(probeRoot, probeTurnHist, "GROKEX_LIVE_TOOL_OK", 8),
		1,
	)

	if !verdict.OK() {
		t.Fatalf("verdict failed: %s (%s) at %s", verdict.Failure, verdict.FailureCategory, verdict.LastProvenStage)
	}
	want := map[string]any{
		"encrypted_reasoning_observed": true,
		"evidence_source":              "canonical_session",
		"history_response_assertion":   "exact_match",
		"response_assertion":           "exact_match",
		"runner_turn_submission_count": 2,
		"same_thread_history":          "completed",
		"status":                       "completed",
		"tool_continuation":            "completed",
	}
	if !reflect.DeepEqual(verdict.Assertions, want) {
		t.Fatalf("assertions = %#v", verdict.Assertions)
	}
	if verdict.Diagnostics["tool_request_count"] != 1 || verdict.Diagnostics["first_encrypted_reasoning_items"] != 2 {
		t.Fatalf("diagnostics = %#v", verdict.Diagnostics)
	}
}

func TestContinuationRequiresAnsweredToolRequest(t *testing.T) {
	graph := fixtures(t, "probe")
	verdict := Continuation(graph,
		completedRun(probeRoot, probeTurnOne, "GROKEX_LIVE_RESPONSE_OK", 1),
		completedRun(probeRoot, probeTurnHist, "GROKEX_LIVE_TOOL_OK", 1),
		0,
	)
	if verdict.OK() || verdict.LastProvenStage != "first_turn_context_verified" || verdict.FailureCategory != "semantic_contract" {
		t.Fatalf("verdict = %+v", verdict)
	}
}

func TestContinuationRequiresEncryptedReasoning(t *testing.T) {
	graph := fixtures(t, "probe")
	root, _ := graph.Session(probeRoot)
	first, _ := root.Turn(probeTurnOne)
	first.EncryptedReasoningCount = 0
	verdict := Continuation(graph,
		completedRun(probeRoot, probeTurnOne, "GROKEX_LIVE_RESPONSE_OK", 1),
		completedRun(probeRoot, probeTurnHist, "GROKEX_LIVE_TOOL_OK", 1),
		1,
	)
	if verdict.OK() || verdict.LastProvenStage != "tool_output_persisted" {
		t.Fatalf("verdict = %+v", verdict)
	}
	if _, asserted := verdict.Assertions["encrypted_reasoning_observed"]; asserted {
		t.Fatal("failed contract must not be asserted")
	}
}

func TestContinuationHistoryMustReturnToolOutput(t *testing.T) {
	graph := fixtures(t, "probe")
	verdict := Continuation(graph,
		completedRun(probeRoot, probeTurnOne, "GROKEX_LIVE_RESPONSE_OK", 1),
		completedRun(probeRoot, probeTurnHist, "GROKEX_LIVE_TOOL_OK", 1),
		1,
	)
	if !verdict.OK() {
		t.Fatalf("baseline should pass: %+v", verdict)
	}
	root, _ := graph.Session(probeRoot)
	history, _ := root.Turn(probeTurnHist)
	history.LastAgentMessage = "I do not remember"
	verdict = Continuation(graph,
		completedRun(probeRoot, probeTurnOne, "GROKEX_LIVE_RESPONSE_OK", 1),
		completedRun(probeRoot, probeTurnHist, "I do not remember", 1),
		1,
	)
	if verdict.OK() || verdict.LastProvenStage != "history_turn_completed" {
		t.Fatalf("verdict = %+v", verdict)
	}
}
