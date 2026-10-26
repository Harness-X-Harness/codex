package live_test

import (
	"bufio"
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"image/png"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

func TestAttachedImageHistory(t *testing.T) {
	for _, mode := range []string{"success", "inline", "field_order", "padded_values", "harmless_commentary", "wrong_first", "wrong_second", "leaked_second", "failed", "wrong_thread", "missing_image_history", "duplicate_turn", "late_first_failure", "commentary_leak", "color_leak", "shape_leak", "duplicate_prior", "prior_failed", "missing_prior", "null_color", "null_shape", "missing_field", "extra_field", "duplicate_key", "wrong_type", "unstructured"} {
		t.Run(mode, func(t *testing.T) {
			options := fixtureOptions(t, "attached:"+mode)
			options.Model = providerfixture.PinnedModel
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			got, err := live.AttachedImageHistory(ctx, options)
			success := mode == "success" || mode == "inline" || mode == "field_order" || mode == "padded_values" || mode == "harmless_commentary" || mode == "missing_image_history"
			if success && (err != nil || !got.Completed || !got.FirstCompleted || !got.FirstRecall || !got.Recalled || got.Turns != 2 || got.Threads != 1) {
				t.Fatalf("missing image history proof: %+v %v", got, err)
			}
			if !success && (err == nil || got.Completed) {
				t.Fatalf("false proof accepted: %+v", got)
			}
			trace := editTrace(t, options)
			if !strings.Contains(trace, "image_input_verified\n") {
				t.Fatal("actual input image was not verified")
			}
			if success && strings.Count(trace, "structured_schema_verified\n") != 2 {
				t.Fatal("formal schema missing from actual request")
			}
			if mode == "null_color" || mode == "null_shape" || mode == "missing_field" || mode == "extra_field" || mode == "duplicate_key" || mode == "wrong_type" || mode == "unstructured" {
				if strings.Count(trace, "turn/start\n") != 1 {
					t.Fatal("invalid or unknown visual reply triggered extra turn")
				}
				code := "visual_answer_invalid"
				if mode == "null_color" || mode == "null_shape" {
					code = "visual_answer_mismatch"
				}
				if live.DescribeFailure(err).Code != code {
					t.Fatalf("wrong visual reply failure: %v", err)
				}
			}
			if success && !strings.Contains(trace, "text_only_followup\n") {
				t.Fatal("followup resupplied image or changed thread")
			}
		})
	}
}

// Offline App Server double: validates the actual SDK UserInput.Image bytes.
// It is only a gate prerequisite, never evidence of real visual understanding.
func fakeAttachedImageServer() {
	var script struct{ Mode, Trace string }
	if json.Unmarshal([]byte(os.Getenv("GROK_API_KEY")), &script) != nil {
		return
	}
	mode := strings.TrimPrefix(script.Mode, "attached:")
	trace, err := os.OpenFile(script.Trace, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0600)
	if err != nil {
		return
	}
	defer trace.Close()
	log := func(s string) { _, _ = io.WriteString(trace, s+"\n") }
	scanner, output := bufio.NewScanner(os.Stdin), json.NewEncoder(os.Stdout)
	scanner.Buffer(make([]byte, 4096), 1<<20)
	cwd, _ := os.Getwd()
	var turns []any
	for scanner.Scan() {
		var request struct {
			ID     json.RawMessage
			Method string
			Params json.RawMessage
		}
		if json.Unmarshal(scanner.Bytes(), &request) != nil {
			return
		}
		log(request.Method)
		var result any
		switch request.Method {
		case "initialize":
			result = map[string]any{"userAgent": "attached-fixture", "codexHome": os.Getenv("CODEX_HOME"), "platformFamily": "unix", "platformOs": runtime.GOOS}
		case "initialized":
			continue
		case "thread/start":
			result = map[string]any{"model": providerfixture.PinnedModel, "modelProvider": "grok", "cwd": cwd, "approvalPolicy": "never", "approvalsReviewer": "user", "sandbox": map[string]any{"type": "readOnly"}, "thread": shippedProtocolThread(map[string]any{"id": "attached-thread", "model": providerfixture.PinnedModel})}
		case "turn/start":
			var params struct {
				ThreadID     string `json:"threadId"`
				Input        []map[string]any
				OutputSchema json.RawMessage
			}
			if json.Unmarshal(request.Params, &params) != nil || params.ThreadID != "attached-thread" {
				return
			}
			var schema, expectedSchema any
			_ = json.Unmarshal([]byte(`{"type":"object","properties":{"color":{"type":["string","null"]},"shape":{"type":["string","null"]}},"required":["color","shape"],"additionalProperties":false}`), &expectedSchema)
			if json.Unmarshal(params.OutputSchema, &schema) != nil || !reflect.DeepEqual(schema, expectedSchema) {
				return
			}
			log("structured_schema_verified")
			index := len(turns)
			if index == 0 {
				if len(params.Input) != 2 || params.Input[0]["type"] != "text" || params.Input[1]["type"] != "image" {
					return
				}
				url, _ := params.Input[1]["url"].(string)
				raw, err := base64.StdEncoding.DecodeString(strings.TrimPrefix(url, "data:image/png;base64,"))
				if err != nil {
					return
				}
				picture, err := png.Decode(bytes.NewReader(raw))
				if err != nil || picture.Bounds().Dx() != 480 || picture.Bounds().Dy() != 240 {
					return
				}
				for _, word := range []string{"red", "circle", "blue", "square"} {
					if strings.Contains(fmt.Sprint(params.Input[0]["text"]), word) {
						return
					}
				}
				log("image_input_verified")
			} else {
				if len(params.Input) != 1 || params.Input[0]["type"] != "text" {
					return
				}
				log("text_only_followup")
			}
			answer := `{"color":"red","shape":"circle"}`
			if index == 1 {
				answer = `{"color":"blue","shape":"square"}`
			}
			if mode == "wrong_first" && index == 0 || mode == "wrong_second" && index == 1 {
				answer = `{"color":"green","shape":"triangle"}`
			}
			if mode == "leaked_second" && index == 0 {
				answer = `{"color":"red","shape":"circle","other":"blue square"}`
			}
			if mode == "field_order" {
				answer = `{"shape":"circle","color":"red"}`
				if index == 1 {
					answer = `{"shape":"square","color":"blue"}`
				}
			}
			if mode == "padded_values" {
				answer = ` {"color":" RED ","shape":"Circle"} `
				if index == 1 {
					answer = ` {"color":" BLUE ","shape":"Square"} `
				}
			}
			invalid := map[string]string{
				"null_color":    `{"color":null,"shape":"circle"}`,
				"null_shape":    `{"color":"red","shape":null}`,
				"missing_field": `{"color":"red"}`,
				"extra_field":   `{"color":"red","shape":"circle","extra":"unknown"}`,
				"duplicate_key": `{"color":"red","color":"green","shape":"circle"}`,
				"wrong_type":    `{"color":true,"shape":"circle"}`,
				"unstructured":  "a red circle",
			}
			if value, ok := invalid[mode]; ok {
				answer = value
			}

			id := fmt.Sprintf("turn-%d", index)
			if mode == "duplicate_turn" {
				id = "turn-0"
			}
			status := "completed"
			if mode == "failed" {
				status = "failed"
			}
			content := params.Input
			if mode == "missing_image_history" && index == 1 {
				turns[0].(map[string]any)["items"].([]any)[0].(map[string]any)["content"] = []any{map[string]any{"type": "text", "text": "attachment omitted"}}
			}
			terminal := map[string]any{"id": id, "status": status, "error": nil, "items": []any{map[string]any{"type": "userMessage", "id": "user-" + id, "content": content}, map[string]any{"type": "agentMessage", "id": "answer-" + id, "phase": "final_answer", "text": answer}}}
			if index == 0 {
				commentary := map[string]string{"commentary_leak": "blue square", "color_leak": "The other object is blue.", "shape_leak": "The other shape is a square.", "harmless_commentary": "I will inspect the attached picture."}[mode]
				if commentary != "" {
					terminal["items"] = append(terminal["items"].([]any), map[string]any{"type": "agentMessage", "id": "commentary", "phase": "commentary", "text": commentary})
				}
			}
			turns = append(turns, terminal)
			if mode == "inline" {
				result = map[string]any{"turn": terminal}
			} else {
				result = map[string]any{"turn": map[string]any{"id": id, "status": "inProgress", "items": []any{}, "error": nil}}
				_ = output.Encode(map[string]any{"id": request.ID, "result": result})
				if mode == "late_first_failure" && index == 1 {
					_ = output.Encode(map[string]any{"method": "turn/completed", "params": map[string]any{"threadId": "attached-thread", "turn": map[string]any{"id": "turn-0", "status": "failed", "items": []any{}, "error": nil}}})
				}
				thread := "attached-thread"
				if mode == "wrong_thread" {
					thread = "other-thread"
				}
				_ = output.Encode(map[string]any{"method": "turn/completed", "params": map[string]any{"threadId": thread, "turn": terminal}})
				continue
			}
		case "thread/read":
			history := append([]any(nil), turns...)
			if len(history) == 2 {
				switch mode {
				case "duplicate_prior":
					history = append(history, history[0])
				case "prior_failed":
					history[0].(map[string]any)["status"] = "failed"
				case "missing_prior":
					history = history[1:]
				}
			}
			result = map[string]any{"thread": shippedProtocolThread(map[string]any{"id": "attached-thread", "model": providerfixture.PinnedModel, "turns": history})}
		default:
			return
		}
		_ = output.Encode(map[string]any{"id": request.ID, "result": result})
	}
}

func TestAttachedImageDiagnosticsSurviveSemanticFailure(t *testing.T) {
	for _, mode := range []string{"success", "wrong_first", "wrong_second", "commentary_leak", "null_color", "extra_field"} {
		t.Run(mode, func(t *testing.T) {
			options := fixtureOptions(t, "attached:"+mode)
			options.Model = providerfixture.PinnedModel
			dir := t.TempDir()
			options.Diagnostics = live.NewDiagnostics(dir, options.APIKey)
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			_, err := live.AttachedImageHistory(ctx, options)
			options.Diagnostics.Close()
			if (mode == "success") != (err == nil) {
				t.Fatalf("diagnostics changed verdict: %v", err)
			}
			events, readErr := os.ReadFile(filepath.Join(dir, "events.jsonl"))
			if readErr != nil {
				t.Fatal(readErr)
			}
			text := string(events)
			expected := `\"color\":\"red\"`
			if mode == "wrong_first" || mode == "wrong_second" {
				expected = `\"color\":\"green\"`
			}
			if mode == "commentary_leak" {
				expected = "blue square"
			}
			if mode == "null_color" {
				expected = `\"color\":null`
			}
			if !strings.Contains(text, expected) || !strings.Contains(text, "attached_image_reply") {
				t.Fatal("missing public output explaining rejection")
			}
			if _, err := os.ReadFile(filepath.Join(dir, "attached-input.png")); err != nil {
				t.Fatal("actual input pixels missing", err)
			}
			if !strings.Contains(text, "output_schema") {
				t.Fatal("formal schema diagnostics missing")
			}
			if mode != "commentary_leak" && !strings.Contains(text, "attached_image_verdict") {
				t.Fatal("parse/semantic diagnostic missing")
			}
			if !strings.Contains(text, "Look at the attached picture.") {
				t.Fatal("actual prompt missing")
			}
			if strings.Contains(text, options.APIKey) {
				t.Fatal("secret leaked in diagnostics")
			}
		})
	}
}
