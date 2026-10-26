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
			if !got.Completed || !got.GenerationRecognized || !got.EditRecognized || !got.FirstCompleted || !got.TurnCompleted || got.Turns != 4 || got.Threads != 3 || got.Images != 2 || got.Artifacts != 2 {
				t.Fatalf("missing visual proof: %+v", got)
			}
			trace := editTrace(t, options)
			for operation, count := range map[string]int{"process": 1, "thread/start": 3, "turn/start": 4, "independent_pixels_verified": 2, "boolean_schema_verified": 2} {
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
	for _, mode := range []string{"missing_image", "bad_payload", "missing_artifact", "failed_turn", "edit_failed", "wrong_first", "wrong_edit", "negated", "judge_null", "judge_string", "judge_number", "judge_object", "judge_extra", "judge_malformed", "judge_failed", "wrong_thread", "mismatched_artifact", "completed_error", "late_failed", "reused_turn"} {
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
			if count < 1 || count > 4 {
				t.Fatal("scenario retried")
			}
			if (strings.HasPrefix(mode, "judge_") || mode == "wrong_first" || mode == "negated") && count != 2 {
				t.Fatal("invalid judge response retried or edit submitted")
			}
			if strings.HasPrefix(mode, "judge_") && mode != "judge_failed" && live.DescribeFailure(err).Code != "image_judge_response_invalid" {
				t.Fatalf("malformed judge response misclassified: %v", err)
			}
			if (mode == "wrong_first" || mode == "wrong_edit" || mode == "negated") && live.DescribeFailure(err).Code != "image_visual_result_mismatch" {
				t.Fatalf("false judge result misclassified: %v", err)
			}
			if mode == "wrong_edit" && (!got.GenerationRecognized || got.EditRecognized) {
				t.Fatal("partial visual evidence lost or manufactured")
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
			expected := `"reply":"true"`
			if mode == "wrong_first" {
				expected = `"reply":"false"`
			}
			if mode == "wrong_edit" {
				expected = `"reply":"false"`
			}
			if mode == "judge_null" {
				expected = `"reply":"null"`
			}
			if !strings.Contains(text, expected) || !strings.Contains(text, "image_recognition") || !strings.Contains(text, "image_verdict") || !strings.Contains(text, `"output_schema":{"type":"boolean"}`) {
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
			if mode == "wrong_first" && (got.Stage != "image_generation_recognition_completed" || got.Turns != 2) {
				t.Fatalf("wrong failed phase or extra turn: %+v", got)
			}
			if strings.Contains(text, "PRIVATE_IMAGE_CANARY") || strings.Contains(text, options.APIKey) {
				t.Fatal("secret leaked in diagnostics")
			}
		})
	}
}
