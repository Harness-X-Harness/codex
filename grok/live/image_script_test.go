package live_test

import (
	"bufio"
	"bytes"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"
)

func imageBoundThread(id, cwd, model string, turns []any) map[string]any {
	if turns == nil {
		turns = []any{}
	}
	return map[string]any{"id": id, "sessionId": "private-session", "modelProvider": "grok", "model": model, "cliVersion": "image-fixture", "createdAt": 1, "updatedAt": 1, "cwd": cwd, "ephemeral": false, "preview": "", "projectId": nil, "source": "appServer", "status": map[string]any{"type": "idle"}, "turns": turns}
}

// This public-stdio double verifies the gate, not actual dog/hat recognition.
func fakeImageServer() {
	var script struct{ Mode, Trace string }
	if json.Unmarshal([]byte(os.Getenv("GROK_API_KEY")), &script) != nil {
		return
	}
	mode := strings.TrimPrefix(script.Mode, "image:")
	trace, err := os.OpenFile(script.Trace, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
	if err != nil {
		return
	}
	defer trace.Close()
	log := func(s string) { _, _ = io.WriteString(trace, s+"\n") }
	log("process")
	scanner, out := bufio.NewScanner(os.Stdin), json.NewEncoder(os.Stdout)
	scanner.Buffer(make([]byte, 4096), 2<<20)
	home, cwd := os.Getenv("CODEX_HOME"), ""
	model := ""
	root := filepath.Join(home, "generated_images")
	_ = os.MkdirAll(root, 0700)
	history := map[string][]any{}
	threads, generated, turns := 0, 0, 0
	imageData := ""
	for scanner.Scan() {
		var req struct {
			ID     json.RawMessage
			Method string
			Params json.RawMessage
		}
		if json.Unmarshal(scanner.Bytes(), &req) != nil {
			return
		}
		log(req.Method)
		var result any
		switch req.Method {
		case "initialize":
			result = map[string]any{"userAgent": "fixture", "codexHome": home, "platformFamily": "unix", "platformOs": runtime.GOOS}
		case "initialized":
			continue
		case "thread/start":
			var p struct {
				Cwd, Model            string
				ExperimentalRawEvents bool
			}
			_ = json.Unmarshal(req.Params, &p)
			if p.ExperimentalRawEvents {
				return
			}
			cwd = p.Cwd
			model = p.Model
			threads++
			id := fmt.Sprintf("thread-%d", threads)
			result = map[string]any{"model": p.Model, "modelProvider": "grok", "cwd": cwd, "approvalPolicy": "never", "approvalsReviewer": "user", "sandbox": map[string]any{"type": "readOnly"}, "thread": imageBoundThread(id, cwd, model, nil)}
		case "turn/start":
			var p struct {
				ThreadID     string
				OutputSchema json.RawMessage
				Input        []struct{ Type, Text, URL string }
			}
			_ = json.Unmarshal(req.Params, &p)
			turns++
			id := fmt.Sprintf("turn-%d", turns)
			items := []any{}
			status := "completed"
			if p.ThreadID == "thread-1" {
				generated++
				if generated > 2 {
					return
				}
				if len(p.Input) != 1 || p.Input[0].Type != "text" || len(p.OutputSchema) != 0 && string(p.OutputSchema) != "null" {
					return
				}
				if generated == 1 && !strings.Contains(p.Input[0].Text, "dog") {
					return
				}
				if generated == 2 && !strings.Contains(p.Input[0].Text, "red hat") {
					return
				}
				canvas := image.NewRGBA(image.Rect(0, 0, 2, 2))
				canvas.SetRGBA(0, 0, color.RGBA{R: uint8(generated * 40), A: 255})
				var b bytes.Buffer
				_ = png.Encode(&b, canvas)
				imageData = base64.StdEncoding.EncodeToString(b.Bytes())
				path := filepath.Join(root, fmt.Sprintf("result-%d.png", generated))
				_ = os.WriteFile(path, b.Bytes(), 0600)
				if mode == "bad_payload" {
					imageData = "bad"
				}
				if mode == "mismatched_artifact" {
					_ = os.WriteFile(path, []byte("not the image"), 0600)
				}
				if mode == "missing_artifact" {
					_ = os.Remove(path)
				}
				if mode != "missing_image" {
					items = append(items, map[string]any{"type": "imageGeneration", "id": fmt.Sprintf("image-%d", generated), "status": "completed", "result": imageData, "savedPath": path})
				}
				if mode == "recovered_invalid_image" {
					items = append([]any{map[string]any{"type": "imageGeneration", "id": "earlier-invalid", "status": "completed", "result": "invalid", "savedPath": path}}, items...)
				}
				if mode == "recovery" || mode == "auxiliary" {
					items = append([]any{map[string]any{"type": "imageGeneration", "id": "failed-attempt", "status": "failed", "result": "", "savedPath": nil}}, items...)
				}
				if mode == "failed_turn" || mode == "edit_failed" && generated == 2 {
					status = "failed"
				}
			} else {
				question := "Is there a dog in the photo? Return only true or false."
				if generated == 2 {
					question = "Is there a dog wearing a red hat in the photo? Return only true or false."
				}
				if len(p.Input) != 2 || p.Input[0].Type != "image" || p.Input[0].URL != "data:image/png;base64,"+imageData || p.Input[1].Text != question {
					return
				}
				var schema map[string]any
				if json.Unmarshal(p.OutputSchema, &schema) != nil || len(schema) != 1 || schema["type"] != "boolean" {
					return
				}
				log("independent_pixels_verified")
				log("boolean_schema_verified")
				reply := "true"
				if mode == "judge_failed" {
					status = "failed"
				}
				if mode == "wrong_first" && generated == 1 || mode == "wrong_edit" && generated == 2 || mode == "negated" {
					reply = "false"
				}
				if mode == "judge_null" {
					reply = "null"
				}
				items = append(items, map[string]any{"type": "agentMessage", "id": "answer", "text": reply, "phase": "final_answer"})
			}
			if mode == "reused_turn" && p.ThreadID == "thread-1" && generated == 2 {
				id = "turn-1"
				history[p.ThreadID] = nil
			}
			terminal := map[string]any{"id": id, "status": status, "items": items, "error": nil}
			if mode == "completed_error" {
				terminal["error"] = map[string]any{"message": "PRIVATE_IMAGE_CANARY", "codexErrorInfo": "other"}
			}
			history[p.ThreadID] = append(history[p.ThreadID], terminal)
			if mode == "inline" {
				result = map[string]any{"turn": terminal}
			} else {
				result = map[string]any{"turn": map[string]any{"id": id, "status": "inProgress", "items": []any{}, "error": nil}}
				_ = out.Encode(map[string]any{"id": req.ID, "result": result})
				thread := p.ThreadID
				if mode == "wrong_thread" {
					thread = "unrelated"
				}
				_ = out.Encode(map[string]any{"method": "turn/completed", "params": map[string]any{"threadId": thread, "turn": terminal}})
				continue
			}
		case "thread/read":
			var p struct{ ThreadID string }
			_ = json.Unmarshal(req.Params, &p)
			result = map[string]any{"thread": imageBoundThread(p.ThreadID, cwd, model, history[p.ThreadID])}
			if mode == "late_failed" {
				_ = out.Encode(map[string]any{"id": req.ID, "result": result})
				_ = out.Encode(map[string]any{"method": "turn/completed", "params": map[string]any{"threadId": p.ThreadID, "turn": map[string]any{"id": fmt.Sprintf("turn-%d", turns), "status": "failed", "items": []any{}, "error": nil}}})
				continue
			}
		default:
			return
		}
		_ = out.Encode(map[string]any{"id": req.ID, "result": result})
	}
}
