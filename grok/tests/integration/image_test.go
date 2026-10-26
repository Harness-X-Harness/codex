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
	integrationObservation(t)
	subject := nativeSubject(t)
	first := imageFixture(t, "testdata/blue.png")
	second := imageFixture(t, "testdata/green.png")
	if first == second {
		t.Fatal("png fixtures are not distinct")
	}
	fixture := &imageNativeHTTP{model: providerfixture.PinnedModel, first: first, second: second}
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
	if err != nil || fixture.failure != "" || fixture.responses != 6 || fixture.recognitions != 2 || fixture.generations != 1 || fixture.edits != 1 || strings.Contains(string(encoded), "PRIVATE_") {
		t.Fatalf("native image edit: %+v; %v", got, err)
	}
	if !got.Completed || !got.FirstCompleted || !got.TurnCompleted || !got.Bound || !got.GenerationRecognized || !got.EditRecognized || got.Images != 2 || got.Artifacts != 2 || got.FirstImageSHA256 == "" || got.EditedImageSHA256 == "" || got.FirstImageSHA256 == got.EditedImageSHA256 || got.Model != options.Model {
		t.Fatal("native image edit evidence was not proven")
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
