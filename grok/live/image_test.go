package live_test

import (
	"context"
	"encoding/json"
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
			for operation, count := range map[string]int{"process": 1, "thread/start": 3, "turn/start": 4, "independent_pixels_verified": 2} {
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
	for _, mode := range []string{"missing_image", "bad_payload", "missing_artifact", "failed_turn", "edit_failed", "wrong_first", "wrong_edit", "negated", "wrong_thread", "mismatched_artifact", "completed_error", "late_failed", "reused_turn"} {
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
			if mode == "wrong_edit" && (!got.GenerationRecognized || got.EditRecognized) {
				t.Fatal("partial visual evidence lost or manufactured")
			}
		})
	}
}
