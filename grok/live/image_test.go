package live_test

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

// Callable deterministic scenario; public scripts are prerequisites only.
func TestGrokImageGenerationEdit(t *testing.T) {
	for _, mode := range []string{"success", "early", "duplicate_terminal", "path_selector", "pinned", "summary_only", "recovered_failure", "null_generation_selectors", "prepared_rendition", "recovered_validation_failure"} {
		t.Run(mode, func(t *testing.T) {
			options := fixtureOptions(t, "image:"+mode)
			if mode == "pinned" {
				options.Model = providerfixture.PinnedModel
			}
			ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
			defer cancel()
			got, err := live.ImageGenerationEdit(ctx, options)
			if err != nil {
				t.Fatal(err)
			}
			wantCalls := 2
			if mode == "recovered_failure" {
				wantCalls = 3
			}
			if mode == "recovered_validation_failure" {
				wantCalls = 5
			}
			if !got.Completed || !got.FirstCompleted || !got.TurnCompleted || !got.Bound || !got.HistorySelected || got.Images != 2 || got.Artifacts != 2 || got.ToolCalls != wantCalls || got.FirstImageSHA256 == "" || got.EditedImageSHA256 == "" || got.FirstImageSHA256 == got.EditedImageSHA256 || got.Model != options.Model {
				t.Fatalf("unexpected safe image evidence: %+v", got)
			}
			if got.FirstHistorySHA256 == "" || got.EditedHistorySHA256 == "" {
				t.Fatal("canonical history digest missing")
			}
			if mode == "prepared_rendition" && (got.FirstHistorySHA256 == got.FirstImageSHA256 || got.EditedHistorySHA256 == got.EditedImageSHA256) {
				t.Fatal("stock preparation was not modeled")
			}
			trace := editTrace(t, options)
			if mode == "recovered_validation_failure" && strings.Count(trace, "validation_text_output\n") != 3 {
				t.Fatal("pre-lifecycle validation/read failures were not consumed")
			}
			assertImageTrace(t, trace, 2, 2)
			assertImageSafe(t, got, "")
		})
	}
}

func TestImageGenerationEditRejectsFalseEvidence(t *testing.T) {
	cases := map[string]string{
		"validation_started":            "live: image call, output or terminal evidence missing",
		"validation_missing_output":     "live: image call, output or terminal evidence missing",
		"validation_wrong_output":       "live: image output precedes canonical call",
		"validation_empty_output":       "live: invalid canonical image output",
		"validation_image_output":       "live: invalid image arguments",
		"validation_success_item":       "live: invalid image arguments",
		"validation_failed_item":        "live: invalid image arguments",
		"validation_conflicting_output": "live: conflicting image output",
		"validation_only":               "live: completed canonical image unavailable",
		"validation_only_edit":          "live: completed canonical image unavailable",
		"validation_valid_only_edit":    "live: completed canonical image unavailable",
		"validation_failed_turn":        "live: image turn did not complete successfully",
		"wrong_thread":                  "live: unrelated image thread evidence",
		"wrong_turn":                    "live: unrelated image turn evidence",
		"wrong_identity":                "live: unexpected image tool identity",
		"wrong_image_id":                "live: image call, output or terminal evidence missing",
		"wrong_output_id":               "live: image output precedes canonical call",
		"missing_call":                  "live: image output precedes canonical call",
		"missing_output":                "live: image call, output or terminal evidence missing",
		"missing_image":                 "live: image call, output or terminal evidence missing",
		"missing_turn":                  "live: protocol ended before proof completion",
		"failed_turn":                   "live: image turn did not complete successfully",
		"interrupted_turn":              "live: image turn did not complete successfully",
		"incomplete_image":              "live: image did not complete successfully",
		"failed_image":                  "live: image did not complete successfully",
		"unrelated_artifact":            "live: image artifact outside save root",
		"preexisting":                   "live: image artifact predates turn",
		"missing_artifact":              "live: image artifact is not a bounded regular file",
		"symlink_artifact":              "live: image artifact is not a bounded regular file",
		"payload_mismatch":              "live: image artifact payload mismatch",
		"malformed_rendition":           "live: image codec did not decode",
		"malformed_codec":               "live: image codec did not decode",
		"jpeg_mislabeled":               "live: invalid image codec or dimensions",
		"wrong_mime":                    "live: invalid canonical image representation",
		"bad_base64":                    "live: invalid image base64",
		"same_output":                   "live: distinct history-derived edit unavailable",
		"missing_reference":             "live: exact image history selector unavailable",
		"invalid_reference":             "live: exact image history selector unavailable",
		"null_reference":                "live: exact image history selector unavailable",
		"string_reference":              "live: invalid image arguments",
		"duplicate_reference":           "live: invalid image arguments",
		"wrong_path":                    "live: exact image history selector unavailable",
		"both_selectors":                "live: exact image history selector unavailable",
		"unrelated_history":             "live: unrelated canonical history image",
		"stale_call":                    "live: stale image call identity",
		"stale_event":                   "live: unrelated image turn evidence",
		"reuse_artifact":                "live: image artifact predates turn",
		"conflicting_terminal":          "live: conflicting image terminal",
		"conflicting_turn":              "live: image turn did not complete successfully",
		"late_conflict":                 "live: conflicting image terminal",
		"read_error":                    "live: protocol request failed",
		"read_wrong_thread":             "live: settled image thread identity unavailable",
		"read_failed":                   "live: settled image turn did not complete",
		"read_missing_image":            "live: image call, output or terminal evidence missing",
		"changed_prior":                 "live: prior image history unavailable",
		"edit_failed":                   "live: image turn did not complete successfully",
		"edit_read_error":               "live: protocol request failed",
		"byte_budget":                   "live: image evidence budget exceeded",
		"call_budget":                   "live: image call budget exceeded",
		"oversized_payload":             "live: protocol frame budget exceeded",
		"conflicting_output":            "live: conflicting image output",
		"changed_original_artifact":     "live: image artifact payload mismatch",
		"read_wrong_turn":               "live: settled image turn did not complete",
		"transparent_false":             "live: invalid image arguments",
		"transparent_true":              "live: invalid image arguments",
		"transparent_null":              "live: invalid image arguments",
		"conflicting_failure":           "live: conflicting image terminal",
		"oversized_dimensions":          "live: image decoded byte budget exceeded",
		"frame_budget":                  "live: image evidence budget exceeded",
	}
	for mode, want := range cases {
		t.Run(mode, func(t *testing.T) {
			options := fixtureOptions(t, "image:"+mode)
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			got, err := live.ImageGenerationEdit(ctx, options)
			if err == nil {
				t.Fatal("false image evidence accepted")
			}
			if err.Error() != want {
				t.Fatalf("wrong rejection boundary: %v; want %s", err, want)
			}
			if got.Completed || got.EditedImageSHA256 != "" {
				t.Fatalf("failure claimed completed image edit: %+v", got)
			}
			trace := editTrace(t, options)
			turns := strings.Count(trace, "turn/start\n")
			if turns < 1 || turns > 2 {
				t.Fatal("unexpected semantic invocation count")
			}
			for operation, count := range map[string]int{"process": 1, "initialize": 1, "thread/start": 1, "image_fixture": 1} {
				if strings.Count("\n"+trace, "\n"+operation+"\n") != count {
					t.Fatal("scenario restarted or skipped binding")
				}
			}
			if turns == 2 && (!got.FirstCompleted || got.FirstImageSHA256 == "" || got.Artifacts < 1) {
				t.Fatal("generation observations lost after edit failure")
			}
			if mode == "edit_failed" && got.TurnCompleted {
				t.Fatal("failed edit reported complete")
			}
			if mode == "edit_read_error" && !got.TurnCompleted {
				t.Fatal("completed edit observation lost after read failure")
			}
			if mode == "payload_mismatch" && (!got.FirstCompleted || !got.TurnCompleted || got.Images != 1 || got.Artifacts != 0 || got.Stage != "image_turn_completed") {
				t.Fatalf("partial image evidence lost: %+v", got)
			}
			if strings.HasPrefix(mode, "validation_") {
				if got.HistorySelected {
					t.Fatal("diagnostic failure established image lineage")
				}
				if mode == "validation_missing_output" || mode == "validation_only" || strings.HasSuffix(mode, "only_edit") {
					if !got.TurnCompleted || !got.FirstCompleted {
						t.Fatal("completed terminal observations lost after diagnostic failure")
					}
				}
			}
			assertImageSafe(t, got, err.Error())
		})
	}
}

func assertImageTrace(t *testing.T, trace string, turns, images int) {
	t.Helper()
	for operation, count := range map[string]int{"process": 1, "initialize": 1, "initialized": 1, "thread/start": 1, "image_fixture": 1, "turn/start": turns, "thread/read": turns, "image_written": images} {
		if strings.Count("\n"+trace, "\n"+operation+"\n") != count {
			t.Fatalf("unexpected %s invocation count", operation)
		}
	}
}
func assertImageSafe(t *testing.T, evidence live.ImageEvidence, message string) {
	t.Helper()
	data, _ := json.Marshal(evidence)
	for _, private := range []string{"PRIVATE_CANARY", "private-", "blue circle", "green", "data:image", "generated_images"} {
		if strings.Contains(string(data)+message, private) {
			t.Fatal("private image evidence escaped")
		}
	}
}
