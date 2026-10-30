package live_test

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/live"
)

func TestGrokImageGenerationEdit(t *testing.T) {
	for _, mode := range []string{"success", "inline", "recovery", "recovered_invalid_image", "auxiliary"} {
		t.Run(mode, func(t *testing.T) {
			options := fixtureOptions(t, "image:"+mode)
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			got, err := live.ImageGenerationEdit(ctx, options)
			if err != nil {
				t.Fatal(err)
			}
			if !got.Completed || !got.GenerationRecognized || !got.EditRecognized || !got.FirstCompleted || !got.TurnCompleted || got.Turns != 3 || got.Threads != 2 || got.Images != 2 || got.Artifacts != 2 {
				t.Fatalf("missing visual proof: %+v", got)
			}
			trace := editTrace(t, options)
			for operation, count := range map[string]int{"process": 1, "thread/start": 2, "turn/start": 3, "ordered_pixels_verified": 1, "paired_schema_verified": 1} {
				if strings.Count(trace, operation+"\n") != count {
					t.Fatalf("unexpected %s invocation count", operation)
				}
			}
			raw, _ := json.Marshal(got)
			if strings.Contains(string(raw), "data:image") || strings.Contains(string(raw), "private-") {
				t.Fatal("private evidence escaped")
			}
		})
	}
}
func TestImageGenerationEditRejectsFalseEvidence(t *testing.T) {
	for _, mode := range []string{"missing_image", "bad_payload", "missing_artifact", "failed_turn", "edit_failed", "wrong_first", "wrong_edit", "negated", "judge_null", "judge_missing", "judge_duplicate", "judge_wrong_type", "judge_extra", "judge_failed", "judge_reused_thread", "judge_wrong_thread", "wrong_thread", "mismatched_artifact", "completed_error", "late_failed", "reused_turn"} {
		t.Run(mode, func(t *testing.T) {
			options := fixtureOptions(t, "image:"+mode)
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			got, err := live.ImageGenerationEdit(ctx, options)
			if err == nil || got.Completed {
				t.Fatalf("false image result accepted: %+v %v", got, err)
			}
			raw, _ := json.Marshal(got)
			if strings.Contains(err.Error()+string(raw), "PRIVATE_IMAGE_CANARY") {
				t.Fatal("private failure escaped")
			}
			count := strings.Count(editTrace(t, options), "turn/start\n")
			if count < 1 || count > 3 {
				t.Fatal("scenario retried")
			}
			if (strings.HasPrefix(mode, "judge_") && mode != "judge_reused_thread" || mode == "wrong_first" || mode == "negated") && count != 3 {
				t.Fatal("judge response retried or ordered pair not submitted")
			}
			if strings.HasPrefix(mode, "judge_") && mode != "judge_failed" && mode != "judge_reused_thread" && mode != "judge_wrong_thread" && live.DescribeFailure(err).Code != "image_judge_response_invalid" {
				t.Fatalf("malformed judge response misclassified: %v", err)
			}
			if (mode == "wrong_first" || mode == "wrong_edit" || mode == "negated") && live.DescribeFailure(err).Code != "image_visual_result_mismatch" {
				t.Fatalf("false judge result misclassified: %v", err)
			}
			if mode == "wrong_edit" && (!got.GenerationRecognized || got.EditRecognized) {
				t.Fatal("partial visual evidence lost or manufactured")
			}
			if mode == "wrong_first" && (got.GenerationRecognized || !got.EditRecognized) {
				t.Fatal("individually valid edit judgment lost")
			}
			if mode == "edit_failed" && (got.GenerationRecognized || got.EditRecognized || got.Images != 1 || got.FirstImageSHA256 == "" || count != 2) {
				t.Fatal("edit failure manufactured recognition or lost first artifact")
			}
		})
	}
}

func TestImageDiagnosticContentSurvivesVerdict(t *testing.T) {
	for _, mode := range []string{"success", "wrong_first", "wrong_edit", "recovery", "judge_null"} {
		t.Run(mode, func(t *testing.T) {
			options := fixtureOptions(t, "image:"+mode)
			dir := t.TempDir()
			options.Diagnostics = live.NewDiagnostics(dir, options.APIKey, "PRIVATE_IMAGE_CANARY")
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			got, err := live.ImageGenerationEdit(ctx, options)
			options.Diagnostics.Close()
			if (mode == "success" || mode == "recovery") != (err == nil) {
				t.Fatalf("diagnostics changed verdict: %v", err)
			}
			events, readErr := os.ReadFile(filepath.Join(dir, "events.jsonl"))
			if readErr != nil {
				t.Fatal(readErr)
			}
			text := string(events)
			expected := `"first_has_one_dog":true,"second_has_two_dogs":true`
			if mode == "wrong_first" {
				expected = `"first_has_one_dog":false,"second_has_two_dogs":true`
			}
			if mode == "wrong_edit" {
				expected = `"first_has_one_dog":true,"second_has_two_dogs":false`
			}
			if mode == "judge_null" {
				expected = `"reply":"null"`
			}
			if !strings.Contains(strings.ReplaceAll(text, `\"`, `"`), expected) || !strings.Contains(text, "image_recognition") || !strings.Contains(text, "image_verdict") || !strings.Contains(text, `"first_has_one_dog":{"type":"boolean"}`) {
				t.Fatal("missing received answer and verdict diagnostics")
			}
			retained := false
			for _, line := range strings.Split(text, "\n") {
				var event struct {
					Kind   string
					Fields struct{ Phase, Artifact string }
				}
				if json.Unmarshal([]byte(line), &event) != nil || event.Kind != "image_attempt" || event.Fields.Phase != "generation" || event.Fields.Artifact == "" {
					continue
				}
				pixels, readErr := os.ReadFile(filepath.Join(dir, event.Fields.Artifact))
				if readErr != nil {
					t.Fatal(readErr)
				}
				sum := sha256.Sum256(pixels)
				if hex.EncodeToString(sum[:]) == got.FirstImageSHA256 {
					retained = true
				}
			}
			if !retained {
				t.Fatal("generated pixels and digest reference were lost on cleanup")
			}
			if got.ReplyBytes == 0 {
				t.Fatal("received visual answer reported as zero bytes")
			}
			if mode == "wrong_first" && (got.Stage != "image_recognition_completed" || got.Turns != 3) {
				t.Fatalf("wrong failed phase or extra turn: %+v", got)
			}
			if strings.Contains(text, "PRIVATE_IMAGE_CANARY") || strings.Contains(text, options.APIKey) {
				t.Fatal("secret leaked in diagnostics")
			}
		})
	}
}

// Optional capture availability must not decide the same business outcome.
func TestImageDiagnosticFailureDoesNotChangeOutcome(t *testing.T) {
	for _, mode := range []string{"success", "wrong_first", "wrong_edit", "edit_failed"} {
		for _, sink := range []string{"closed", "unavailable"} {
			t.Run(mode+"/"+sink, func(t *testing.T) {
				options := fixtureOptions(t, "image:"+mode)
				dir := t.TempDir()
				if sink == "unavailable" {
					dir = filepath.Join(dir, "file")
					if err := os.WriteFile(dir, []byte("not a directory"), 0600); err != nil {
						t.Fatal(err)
					}
				}
				options.Diagnostics = live.NewDiagnostics(dir, options.APIKey)
				if sink == "closed" {
					options.Diagnostics.Close()
				}
				ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
				defer cancel()
				got, err := live.ImageGenerationEdit(ctx, options)
				options.Diagnostics.Close()
				if (err == nil) != (mode == "success") || got.Completed != (mode == "success") {
					t.Fatalf("capture failure changed outcome: %+v, %v", got, err)
				}
				if mode == "wrong_edit" && (!got.GenerationRecognized || got.EditRecognized) {
					t.Fatal("partial result changed")
				}
				if mode == "wrong_first" && (got.GenerationRecognized || !got.EditRecognized) {
					t.Fatal("partial result changed")
				}
				if mode == "edit_failed" && (got.GenerationRecognized || got.EditRecognized || got.Images != 1) {
					t.Fatal("unobserved judgment credited")
				}
			})
		}
	}
}
