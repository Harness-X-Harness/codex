//go:build integration

package integration_test

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
)

// This checks the HTTP fixture's counterexamples without launching Codex. The
// actual-binary owner remains TestImagePinnedFixture; this is not native proof.
func TestImageJudgeFixtureRejectsFalseComposition(t *testing.T) {
	for _, mode := range []string{"ok", "reordered_required", "swapped", "missing", "reused", "producer_history", "producer_prompt", "wrong_question", "missing_schema", "missing_required", "duplicate_required", "wrong_required", "wrong_property", "extra_property", "not_strict", "early", "repeat"} {
		t.Run(mode, func(t *testing.T) {
			// Account for an executed selected test, not a native-runtime proof.
			integrationObservation(t)
			first, second := imageFixture(t, "testdata/blue.png"), imageFixture(t, "testdata/green.png")
			fixture := &imageNativeHTTP{model: providerfixture.PinnedModel, first: first, second: second, generations: 1, edits: 1}
			images := []string{"data:image/png;base64," + first, "data:image/png;base64," + second}
			question := imageJudgeQuestion
			var schema map[string]any
			if err := json.Unmarshal([]byte(imageJudgeSchema), &schema); err != nil {
				t.Fatal(err)
			}
			switch mode {
			case "swapped":
				images[0], images[1] = images[1], images[0]
			case "missing":
				images = images[:1]
			case "reused":
				images[1] = images[0]
			case "producer_prompt":
				question += " Draw a clear picture of exactly one dog on a plain background."
			case "wrong_question":
				question = "What is in these images?"
			case "missing_required":
				schema["required"] = []string{"first_has_one_dog"}
			case "duplicate_required":
				schema["required"] = []string{"first_has_one_dog", "first_has_one_dog"}
			case "wrong_required":
				schema["required"] = []string{"first_has_one_dog", "unknown"}
			case "reordered_required":
				schema["required"] = []string{"second_has_two_dogs", "first_has_one_dog"}
			case "wrong_property":
				schema["properties"].(map[string]any)["second_has_two_dogs"] = map[string]any{"type": "string"}
			case "extra_property":
				schema["properties"].(map[string]any)["other"] = map[string]any{"type": "boolean"}
			case "early":
				fixture.edits = 0
			case "repeat":
				fixture.recognitions = 1
			}
			content := []any{}
			for _, image := range images {
				content = append(content, map[string]any{"type": "input_image", "image_url": image})
			}
			content = append(content, map[string]any{"type": "input_text", "text": question})
			input := []any{map[string]any{"type": "message", "role": "user", "content": content}}
			if mode == "producer_history" {
				input = append(input, map[string]any{"type": "message", "role": "assistant", "content": []any{}})
			}
			format := map[string]any{"type": "json_schema", "strict": true, "schema": schema}
			if mode == "missing_schema" {
				delete(format, "schema")
			}
			if mode == "not_strict" {
				format["strict"] = false
			}
			body, err := json.Marshal(map[string]any{"model": providerfixture.PinnedModel, "stream": true, "store": false, "input": input, "text": map[string]any{"format": format}})
			if err != nil {
				t.Fatal(err)
			}
			request := httptest.NewRequest(http.MethodPost, "/v1/responses", bytes.NewReader(body))
			request.Header.Set("Authorization", "Bearer fixture-key")
			response := httptest.NewRecorder()
			fixture.ServeHTTP(response, request)
			accepted := mode == "ok" || mode == "reordered_required"
			if accepted {
				if fixture.failure != "" || fixture.recognitions != 1 || fixture.responses != 1 || response.Code != http.StatusOK || !strings.Contains(response.Body.String(), "response.completed") {
					t.Fatalf("valid ordered pair rejected: %s", fixture.failure)
				}
			} else if fixture.failure == "" || response.Code == http.StatusOK || fixture.responses != 0 {
				t.Fatal("invalid composition accepted")
			}
		})
	}
}
