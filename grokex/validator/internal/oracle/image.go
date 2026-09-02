package oracle

import (
	"encoding/json"
	"path/filepath"

	"github.com/Harness-X-Harness/codex/grokex/validator/internal/driver"
	"github.com/Harness-X-Harness/codex/grokex/validator/internal/rollout"
)

// Image scenario prompts: one generation Turn, then one edit Turn on the same
// Thread that must consume the first result through history.
const (
	ImageGenerationPrompt = "Generate an image of a blue circle on a plain white background."
	ImageEditPrompt       = "Edit the image you just generated so the circle is green while " +
		"keeping the plain white background."
)

// Image proves grokex-image-generation-history-edit from the session graph and
// the delivered replies:
//
//   - both Turns completed on one root Thread and each delivered a reply;
//   - each Turn persisted a completed image result whose saved file equals the
//     payload, carries a real image codec, and uses the matching extension;
//   - the edit call's arguments reference the generation result through
//     history (recent conversation images or the saved artifact path);
//   - the edited artifact is not byte-identical to the generated one.
func Image(graph *rollout.Graph, generation, edit driver.TurnRun) Verdict {
	verdict, stage := newVerdict()
	verdict.Assertions["evidence_source"] = EvidenceSource
	verdict.Assertions["runner_turn_submission_count"] = 2
	verdict.Diagnostics["session_file_count"] = len(graph.Sessions)
	verdict.Diagnostics["turn_durations_seconds"] = []float64{seconds(generation.Duration), seconds(edit.Duration)}

	root, ok := graph.Session(generation.ThreadID)
	if !stage.require("root_session_found", ok && root.ModelProvider == driver.Provider, "root session missing or not bound to grok", "semantic_contract") {
		return *verdict
	}
	verdict.Diagnostics["root_history_mode"] = root.HistoryMode

	var artifacts [2]Artifact
	var turns [2]*rollout.Turn
	completed, failed := 0, 0
	for index, phase := range []struct {
		name string
		run  driver.TurnRun
	}{{"generation", generation}, {"edit", edit}} {
		if index == 1 {
			if !stage.require("same_thread", edit.ThreadID == generation.ThreadID, "edit Turn ran on another Thread", "semantic_contract") {
				return *verdict
			}
			verdict.Assertions["same_thread"] = true
		}
		turn, ok := root.Turn(phase.run.TurnID)
		verdict.Diagnostics[phase.name+"_delivered_turn_status"] = phase.run.Status
		if ok {
			verdict.Diagnostics[phase.name+"_turn_state"] = turn.State()
			verdict.Diagnostics[phase.name+"_function_calls"] = turn.FunctionCallCounts()
			verdict.Diagnostics[phase.name+"_item_types"] = turn.ItemTypes
			for _, result := range turn.ImageResults {
				switch result.Status {
				case "completed":
					completed++
				case "failed":
					failed++
				}
			}
		}
		verdict.Diagnostics["image_items_completed"] = completed
		verdict.Diagnostics["image_items_failed"] = failed
		if !stage.require(phase.name+"_turn_completed", ok && turn.Completed && !turn.Failed && phase.run.Completed(), phase.name+" Turn did not complete", deadlineOr(phase.run, "semantic_contract")) {
			return *verdict
		}
		verdict.Assertions[phase.name+"_completion"] = "completed"
		replied := turn.LastAgentMessage != "" && phase.run.FinalResponse != ""
		if !stage.require(phase.name+"_reply_delivered", replied, phase.name+" Turn delivered no reply", "delivery") {
			return *verdict
		}
		verdict.Assertions[phase.name+"_agent_reply_seen"] = true
		artifact, err := firstVerifiedArtifact(turn)
		if !stage.require(phase.name+"_artifact_verified", err == nil, phase.name+": "+errString(err), "semantic_contract") {
			return *verdict
		}
		verdict.Assertions[phase.name+"_artifact_match"] = true
		verdict.Assertions[phase.name+"_image_decodable"] = true
		verdict.Assertions[phase.name+"_image_mime"] = artifact.MIME
		verdict.Assertions[phase.name+"_artifact_extension"] = artifact.Extension
		verdict.Diagnostics[phase.name+"_artifact_sha256"] = artifact.SHA256
		verdict.Diagnostics[phase.name+"_artifact_dimensions"] = []int{artifact.Width, artifact.Height}
		artifacts[index], turns[index] = artifact, turn
	}

	call, ok := turns[1].FunctionCall(artifacts[1].CallID)
	if !stage.require("history_arguments_verified", ok && referencesHistory(call.Arguments, artifacts[0].Path), "edit call does not reference the generated image through history", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["history_arguments_verified"] = true
	if !stage.require("edit_artifact_distinct", artifacts[0].SHA256 != artifacts[1].SHA256, "edited image is byte-identical to the generated image", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["edit_artifact_distinct"] = true
	verdict.Assertions["status"] = "completed"
	return *verdict
}

func firstVerifiedArtifact(turn *rollout.Turn) (Artifact, error) {
	var lastErr error
	for _, result := range turn.ImageResults {
		if result.Status != "completed" {
			continue
		}
		artifact, err := VerifyArtifact(result)
		if err == nil {
			return artifact, nil
		}
		lastErr = err
	}
	if lastErr == nil {
		return Artifact{}, errNoCompletedImage
	}
	return Artifact{}, lastErr
}

type staticError string

func (e staticError) Error() string { return string(e) }

const errNoCompletedImage = staticError("no completed image result in the Turn")

func errString(err error) string {
	if err == nil {
		return ""
	}
	return err.Error()
}

// referencesHistory mirrors the imagegen tool contract: an edit consumes prior
// images either as the N most recent conversation images with no explicit
// paths, or by naming the saved artifact path explicitly. Never both.
func referencesHistory(arguments string, generatedPath string) bool {
	var args struct {
		NumLastImagesToInclude *int     `json:"num_last_images_to_include"`
		ReferencedImagePaths   []string `json:"referenced_image_paths"`
	}
	if json.Unmarshal([]byte(arguments), &args) != nil {
		return false
	}
	recent := args.NumLastImagesToInclude != nil && *args.NumLastImagesToInclude > 0 && len(args.ReferencedImagePaths) == 0
	if recent {
		return true
	}
	if args.NumLastImagesToInclude != nil {
		return false
	}
	want := filepath.Clean(generatedPath)
	for _, path := range args.ReferencedImagePaths {
		if filepath.Clean(path) == want {
			return true
		}
	}
	return false
}
