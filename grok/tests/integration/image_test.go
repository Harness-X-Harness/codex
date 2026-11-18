//go:build integration

package integration_test

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"image/png"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

func TestImagePinnedFixture(t *testing.T) {
	for _, tc := range []struct {
		name                                        string
		replies                                     []string
		responses, recognitions, generations, edits int
		rejected, first, second                     bool
	}{
		{"true", nil, 5, 1, 1, 1, false, true, true},
		{"generation_false", []string{`{"first_has_one_dog":false,"second_has_two_dogs":true}`}, 5, 1, 1, 1, true, false, true},
		{"generation_nonboolean", []string{`{"result":true}`}, 5, 1, 1, 1, true, false, false},
		{"edit_false", []string{`{"first_has_one_dog":true,"second_has_two_dogs":false}`}, 5, 1, 1, 1, true, true, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			integrationObservation(t)
			subject := nativeSubject(t)
			first := imageFixture(t, "testdata/blue.png")
			second := imageFixture(t, "testdata/green.png")
			if first == second {
				t.Fatal("png fixtures are not distinct")
			}
			fixture := &imageNativeHTTP{model: providerfixture.PinnedModel, first: first, second: second, judgeReplies: tc.replies}
			backend := httptest.NewServer(fixture)
			defer backend.Close()
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
			defer cancel()
			options := live.Options{Subject: subject, Model: providerfixture.PinnedModel, BaseURL: backend.URL + "/v1", APIKey: "fixture-key"}
			got, err := live.ImageGenerationEdit(ctx, options)
			fixture.mu.Lock()
			defer fixture.mu.Unlock()
			encoded, _ := json.Marshal(got)
			t.Logf("controlled image-pinned: evidence=%s responses=%d generations=%d edits=%d failure=%s tools=%s", encoded, fixture.responses, fixture.generations, fixture.edits, fixture.failure, strings.Join(fixture.toolNames, ","))
			if (err != nil) != tc.rejected || fixture.failure != "" || fixture.responses != tc.responses || fixture.recognitions != tc.recognitions || fixture.generations != tc.generations || fixture.edits != tc.edits || strings.Contains(string(encoded), "PRIVATE_") {
				t.Fatalf("native image edit: %+v; %v", got, err)
			}
			if tc.rejected {
				if got.Completed || got.GenerationRecognized != tc.first || got.EditRecognized != tc.second {
					t.Fatalf("false judge outcome accepted: %+v", got)
				}
				return
			}
			if !got.Completed || !got.FirstCompleted || !got.TurnCompleted || !got.Bound || !got.GenerationRecognized || !got.EditRecognized || got.Images != 2 || got.Artifacts != 2 || got.FirstImageSHA256 == "" || got.EditedImageSHA256 == "" || got.Model != options.Model {
				t.Fatal("native image edit evidence was not proven")
			}
		})
	}
}

func imageFixture(t *testing.T, path string) string {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal("immutable image fixture unavailable")
	}
	image, err := png.Decode(bytes.NewReader(data))
	if err != nil || image.Bounds().Dx() != 1 || image.Bounds().Dy() != 1 {
		t.Fatal("immutable image fixture is not a one-pixel PNG")
	}
	return base64.StdEncoding.EncodeToString(data)
}
