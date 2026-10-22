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
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

func TestAttachedImageHistory(t *testing.T) {
	for _, mode := range []string{"success", "inline", "wrong_first", "wrong_second", "leaked_second", "failed", "wrong_thread", "missing_image_history", "duplicate_turn", "late_first_failure", "commentary_leak"} {
		t.Run(mode, func(t *testing.T) {
			options := fixtureOptions(t, "attached:"+mode)
			options.Model = providerfixture.PinnedModel
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			got, err := live.AttachedImageHistory(ctx, options)
			success := mode == "success" || mode == "inline"
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
				ThreadID string `json:"threadId"`
				Input    []map[string]any
			}
			if json.Unmarshal(request.Params, &params) != nil || params.ThreadID != "attached-thread" {
				return
			}
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
			answer := "red circle"
			if index == 1 {
				answer = "blue square"
			}
			if mode == "wrong_first" && index == 0 || mode == "wrong_second" && index == 1 {
				answer = "green triangle"
			}
			if mode == "leaked_second" && index == 0 {
				answer = "red circle blue square"
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
			if mode == "commentary_leak" && index == 0 {
				terminal["items"] = append(terminal["items"].([]any), map[string]any{"type": "agentMessage", "id": "commentary", "phase": "commentary", "text": "blue square"})
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
			result = map[string]any{"thread": shippedProtocolThread(map[string]any{"id": "attached-thread", "model": providerfixture.PinnedModel, "turns": turns})}
		default:
			return
		}
		_ = output.Encode(map[string]any{"id": request.ID, "result": result})
	}
}
