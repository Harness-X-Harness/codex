package live_test

import (
	"bufio"
	"bytes"
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"hash/crc32"
	"image"
	"image/color"
	"image/draw"
	"image/jpeg"
	"image/png"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"runtime"
	"strings"
)

// The script models public protocol and file effects. It never runs native
// image generation, uses a backend, or establishes the circle's visual meaning.
func imageBoundThread(id, cwd string, turns []any) map[string]any {
	if turns == nil {
		turns = []any{}
	}
	return map[string]any{
		"id": id, "sessionId": "private-session", "modelProvider": "grok", "cliVersion": "image-fixture",
		"createdAt": 1, "updatedAt": 1, "cwd": cwd, "ephemeral": false, "preview": "", "projectId": nil,
		"source": "appServer", "status": map[string]any{"type": "idle"}, "turns": turns,
	}
}

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
	signal.Notify(make(chan os.Signal, 1), os.Interrupt)
	log := func(value string) { _, _ = io.WriteString(trace, value+"\n") }
	log("process")
	input, output := bufio.NewScanner(os.Stdin), json.NewEncoder(os.Stdout)
	send := func(method string, params any) { _ = output.Encode(map[string]any{"method": method, "params": params}) }
	home := os.Getenv("CODEX_HOME")
	root := filepath.Join(home, "generated_images", "private-thread")
	if os.MkdirAll(root, 0700) != nil {
		return
	}
	pictureSized := func(green bool, side int) []byte {
		canvas := image.NewNRGBA(image.Rect(0, 0, side, side))
		pixel := color.NRGBA{B: 255, A: 255}
		if green {
			pixel = color.NRGBA{G: 255, A: 255}
		}
		draw.Draw(canvas, canvas.Bounds(), &image.Uniform{C: pixel}, image.Point{}, draw.Src)
		var data bytes.Buffer
		if mode == "jpeg_mislabeled" {
			_ = jpeg.Encode(&data, canvas, nil)
		} else {
			_ = png.Encode(&data, canvas)
		}
		return data.Bytes()
	}
	picture := func(green bool) []byte {
		side := 2
		if mode == "prepared_rendition" {
			side = 2048
		}
		return pictureSized(green, side)
	}
	firstPath := filepath.Join(root, "private-call-1.png")
	if mode == "preexisting" {
		_ = os.WriteFile(firstPath, picture(false), 0600)
	}
	var turns []any
	for input.Scan() {
		var request struct {
			ID     json.RawMessage
			Method string
			Params json.RawMessage
		}
		if json.Unmarshal(input.Bytes(), &request) != nil {
			return
		}
		log(request.Method)
		var result any
		switch request.Method {
		case "initialize":
			family := "unix"
			if runtime.GOOS == "windows" {
				family = "windows"
			}
			result = map[string]any{"userAgent": "image-fixture", "codexHome": home, "platformFamily": family, "platformOs": runtime.GOOS}
		case "initialized":
			continue
		case "thread/start":
			var params struct {
				Model, Cwd            string
				ExperimentalRawEvents bool
			}
			if json.Unmarshal(request.Params, &params) != nil || !params.ExperimentalRawEvents {
				return
			}
			var catalog struct {
				Models []struct {
					Slug            string
					InputModalities []string `json:"input_modalities"`
					ToolMode        string   `json:"tool_mode"`
				}
			}
			data, err := os.ReadFile(filepath.Join(home, "models.json"))
			if err != nil || json.Unmarshal(data, &catalog) != nil || len(catalog.Models) != 1 || catalog.Models[0].Slug != params.Model || strings.Join(catalog.Models[0].InputModalities, ",") != "text,image" || catalog.Models[0].ToolMode != "direct" {
				return
			}
			config, err := os.ReadFile(filepath.Join(home, "config.toml"))
			if err != nil || !strings.Contains(string(config), "view_image = true") || !strings.Contains(string(config), "image_generation = true") || !strings.Contains(string(config), "omit_app_server_notification_media = false") {
				return
			}
			log("image_fixture")
			result = map[string]any{
				"model": params.Model, "modelProvider": "grok", "cwd": params.Cwd, "approvalPolicy": "never", "approvalsReviewer": "user",
				"sandbox": map[string]any{"type": "readOnly"},
				"thread":  imageBoundThread("private-thread", params.Cwd, []any{}),
			}
		case "turn/start":
			var params struct {
				ThreadID string `json:"threadId"`
				Input    []struct{ Type, Text string }
			}
			if json.Unmarshal(request.Params, &params) != nil || params.ThreadID != "private-thread" || len(params.Input) != 1 || params.Input[0].Type != "text" {
				return
			}
			second := len(turns) == 1
			suffix := "1"
			if second {
				suffix = "2"
			}
			turnID, callID := "private-turn-"+suffix, "private-call-"+suffix
			args := map[string]any{"prompt": "PRIVATE_CANARY"}
			if mode == "null_generation_selectors" && !second {
				args["referenced_image_paths"], args["num_last_images_to_include"] = nil, nil
			}
			if mode == "transparent_false" {
				args["transparent_background"] = false
			}
			if mode == "transparent_true" {
				args["transparent_background"] = true
			}
			if mode == "transparent_null" {
				args["transparent_background"] = nil
			}
			if second {
				args["num_last_images_to_include"] = 1
			}
			if mode == "path_selector" && second {
				delete(args, "num_last_images_to_include")
				args["referenced_image_paths"] = []string{firstPath}
			}
			if second {
				switch mode {
				case "missing_reference":
					delete(args, "num_last_images_to_include")
				case "invalid_reference":
					args["num_last_images_to_include"] = 2
				case "null_reference":
					args["num_last_images_to_include"] = nil
				case "string_reference":
					args["num_last_images_to_include"] = "1"
				case "wrong_path":
					delete(args, "num_last_images_to_include")
					args["referenced_image_paths"] = []string{"PRIVATE_CANARY"}
				case "both_selectors":
					args["referenced_image_paths"] = []string{firstPath}
				case "stale_call":
					callID = "private-call-1"
				}
			}
			encoded, _ := json.Marshal(args)
			arguments := string(encoded)
			if second && mode == "duplicate_reference" {
				arguments = strings.TrimSuffix(arguments, "}") + `,"num_last_images_to_include":0}`
			}
			call := map[string]any{"type": "function_call", "call_id": callID, "name": "imagegen", "namespace": "image_gen", "arguments": arguments}
			if mode == "wrong_identity" {
				call["namespace"] = "PRIVATE_CANARY"
			}
			data := picture(second && mode != "same_output")
			if mode == "oversized_dimensions" {
				binary.BigEndian.PutUint32(data[16:20], 4096)
				binary.BigEndian.PutUint32(data[20:24], 4096)
				binary.BigEndian.PutUint32(data[29:33], crc32.ChecksumIEEE(data[12:29]))
			}
			if mode == "malformed_codec" {
				data = data[:len(data)-12]
			}
			payload := base64.StdEncoding.EncodeToString(data)
			if mode == "bad_base64" {
				payload = "PRIVATE_CANARY"
			}
			path := filepath.Join(root, callID+".png")
			if mode == "unrelated_artifact" {
				path = filepath.Join(home, "unrelated.png")
			}
			if second && mode == "reuse_artifact" {
				path = firstPath
			}
			if mode != "missing_artifact" {
				saved := data
				if mode == "payload_mismatch" {
					saved = picture(true)
				}
				if mode == "symlink_artifact" {
					target := filepath.Join(home, "other.png")
					_ = os.WriteFile(target, saved, 0600)
					_ = os.Symlink(target, path)
				} else {
					_ = os.WriteFile(path, saved, 0600)
				}
				log("image_written")
			}
			imageItem := map[string]any{"type": "imageGeneration", "id": callID, "status": "completed", "result": payload, "savedPath": path, "failure": nil}
			if mode == "incomplete_image" {
				imageItem["status"] = "in_progress"
			}
			if mode == "failed_image" {
				imageItem["status"] = "failed"
			}
			if mode == "wrong_image_id" {
				imageItem["id"] = "private-other-call"
			}
			url := "data:image/png;base64," + payload
			if mode == "prepared_rendition" {
				url = "data:image/png;base64," + base64.StdEncoding.EncodeToString(pictureSized(second, 1600))
			}
			if mode == "wrong_mime" {
				url = "data:image/jpeg;base64," + payload
			}
			toolOutput := map[string]any{"type": "function_call_output", "call_id": callID, "output": []any{map[string]any{"type": "input_image", "image_url": url}, map[string]any{"type": "input_text", "text": "PRIVATE_CANARY"}}}
			if mode == "wrong_output_id" {
				toolOutput["call_id"] = "private-other-call"
			}
			if mode == "malformed_rendition" {
				toolOutput["output"] = []any{map[string]any{"type": "input_image", "image_url": "data:image/png;base64," + base64.StdEncoding.EncodeToString(data[:len(data)-12])}}
			}
			terminal := map[string]any{"id": turnID, "status": "completed", "items": []any{imageItem}}
			if mode == "failed_turn" || mode == "interrupted_turn" || second && mode == "edit_failed" {
				terminal["status"] = "failed"
				terminal["error"] = map[string]any{"message": "PRIVATE_CANARY"}
			}
			if mode == "interrupted_turn" {
				terminal["status"] = "interrupted"
			}
			threadID, eventTurnID := "private-thread", turnID
			if mode == "wrong_thread" {
				threadID = "PRIVATE_CANARY"
			}
			if mode == "wrong_turn" || second && mode == "stale_event" {
				eventTurnID = "private-turn-stale"
			}
			raw := func(item any) {
				send("rawResponseItem/completed", map[string]any{"threadId": threadID, "turnId": eventTurnID, "item": item})
			}
			public := func(item any) {
				send("item/completed", map[string]any{"threadId": threadID, "turnId": eventTurnID, "completedAtMs": 1, "item": item})
			}
			start := func() {
				_ = output.Encode(map[string]any{"id": request.ID, "result": map[string]any{"turn": map[string]any{"id": turnID, "status": "inProgress", "items": []any{}}}})
			}
			if mode != "early" {
				start()
			}
			if mode == "unrelated_notifications" {
				send("turn/completed", map[string]any{"threadId": "other-thread", "turn": map[string]any{"id": "other-turn", "status": "failed", "items": []any{}}})
				send("turn/completed", map[string]any{"threadId": "private-thread", "turn": map[string]any{"id": "uncredited-turn", "status": "failed", "items": []any{}}})
			}
			if second && mode == "late_prior_failure" {
				send("turn/completed", map[string]any{"threadId": "private-thread", "turn": map[string]any{"id": "private-turn-1", "status": "failed", "items": []any{}}})
			}
			view := func() {
				viewPath := firstPath
				if mode == "view_wrong_path" {
					viewPath = filepath.Join(home, "unrelated.png")
				}
				viewArgs, _ := json.Marshal(map[string]any{"path": viewPath})
				raw(map[string]any{"type": "function_call", "call_id": "private-view", "name": "view_image", "arguments": string(viewArgs)})
				viewData := picture(false)
				if mode == "view_wrong_output" {
					viewData = picture(true)
				}
				if mode != "view_missing_output" {
					raw(map[string]any{"type": "function_call_output", "call_id": "private-view", "output": []any{map[string]any{"type": "input_image", "image_url": "data:image/png;base64," + base64.StdEncoding.EncodeToString(viewData)}}})
				}
				if mode == "view_substituted_generation" {
					imageItem["id"] = "private-view"
				}
				log("bound_image_view")
			}
			if second && strings.HasPrefix(mode, "view_") && mode != "view_after_edit" {
				view()
			}
			if second && mode == "retained_image_message" {
				raw(map[string]any{"type": "message", "role": "user", "content": []any{map[string]any{"type": "input_image", "image_url": "data:image/png;base64," + base64.StdEncoding.EncodeToString(picture(false))}}})
			}
			if mode == "neutral_history_metadata" {
				raw(map[string]any{"type": "compaction", "encrypted_content": "PRIVATE_CANARY"})
			}
			if second && mode == "unrelated_history" {
				raw(map[string]any{"type": "message", "role": "user", "content": []any{map[string]any{"type": "input_image", "image_url": "PRIVATE_CANARY"}}})
			}
			if mode == "frame_budget" {
				item := map[string]any{"type": "message", "role": "user", "content": []any{map[string]any{"type": "input_text", "text": "x"}}}
				for range 4097 {
					raw(item)
				}
				continue
			}
			if (mode == "recovered_failure" || mode == "conflicting_failure") && !second {
				failedCall := map[string]any{"type": "function_call", "call_id": "private-failed-call", "name": "imagegen", "namespace": "image_gen", "arguments": arguments}
				failedItem := map[string]any{"type": "imageGeneration", "id": "private-failed-call", "status": "failed", "result": ""}
				raw(failedCall)
				public(failedItem)
				if mode == "conflicting_failure" {
					failedItem["failure"] = map[string]any{"type": "usageLimitExceeded", "limitId": "PRIVATE_CANARY", "resetsAt": 1}
					public(failedItem)
				}
				raw(map[string]any{"type": "function_call_output", "call_id": "private-failed-call", "output": "PRIVATE_CANARY"})
				terminal["items"] = []any{failedItem, imageItem}
			}
			validation := mode == "recovered_validation_failure" || strings.HasPrefix(mode, "validation_")
			if strings.HasSuffix(mode, "only_edit") && !second {
				validation = false
			}
			if validation {
				invalidArguments := `{"prompt":"PRIVATE_CANARY","transparent_background":false}`
				if second {
					invalidArguments = `{"prompt":"PRIVATE_CANARY","num_last_images_to_include":0}`
				}
				validArguments, _ := json.Marshal(map[string]any{"prompt": "PRIVATE_CANARY", "referenced_image_paths": []string{firstPath}})
				if mode == "validation_valid_only_edit" {
					invalidArguments = string(validArguments)
				}
				if mode == "validation_started" {
					invalidArguments = arguments
				}
				invalidID := "private-validation-" + suffix
				raw(map[string]any{"type": "function_call", "call_id": invalidID, "name": "imagegen", "namespace": "image_gen", "arguments": invalidArguments})
				if mode == "validation_started" {
					send("item/started", map[string]any{"threadId": threadID, "turnId": eventTurnID, "startedAtMs": 1, "item": map[string]any{"type": "imageGeneration", "id": invalidID, "status": "in_progress", "result": ""}})
				}
				failedOutput := map[string]any{"type": "function_call_output", "call_id": invalidID, "output": "PRIVATE_CANARY validation failed"}
				switch mode {
				case "validation_wrong_output":
					failedOutput["call_id"] = "private-missing-call"
				case "validation_empty_output":
					failedOutput["output"] = ""
				case "validation_image_output":
					failedOutput["output"] = toolOutput["output"]
				}
				if mode != "validation_missing_output" {
					raw(failedOutput)
					log("validation_text_output")
				}
				if mode == "validation_conflicting_output" {
					failedOutput["output"] = "PRIVATE_CANARY conflicting failure"
					raw(failedOutput)
				}
				if mode == "validation_success_item" || mode == "validation_failed_item" {
					invalidItem := map[string]any{"type": "imageGeneration", "id": invalidID, "status": "completed", "result": payload, "savedPath": path}
					if mode == "validation_failed_item" {
						invalidItem["status"], invalidItem["result"], invalidItem["savedPath"] = "failed", "", ""
					}
					public(invalidItem)
				}
				if mode == "recovered_validation_failure" && second {
					raw(map[string]any{"type": "function_call", "call_id": "private-read-failure", "name": "imagegen", "namespace": "image_gen", "arguments": string(validArguments)})
					raw(map[string]any{"type": "function_call_output", "call_id": "private-read-failure", "output": "PRIVATE_CANARY image read failed"})
					log("validation_text_output")
				}
				if mode == "validation_only" || strings.HasSuffix(mode, "only_edit") {
					terminal["items"] = []any{}
					turns = append(turns, terminal)
					send("turn/completed", map[string]any{"threadId": threadID, "turn": terminal})
					continue
				}
				if mode == "validation_failed_turn" {
					terminal["status"] = "failed"
				}
			}
			if mode == "call_budget" {
				for index := range 65 {
					raw(map[string]any{"type": "function_call", "call_id": strings.Repeat("x", index+1), "name": "imagegen", "namespace": "image_gen", "arguments": arguments})
				}
				continue
			}
			if mode == "oversized_payload" {
				imageItem["result"] = strings.Repeat("A", 17<<20)
			}
			if mode == "byte_budget" {
				// Five schema-valid frames stay under the 16 MiB SDK limit and
				// together cross the 64 MiB observation budget.
				text := strings.Repeat("x", 13<<20)
				for range 5 {
					raw(map[string]any{"type": "message", "role": "user", "content": []any{map[string]any{"type": "input_text", "text": text}}})
				}
				continue
			}
			if mode != "missing_call" {
				raw(call)
			}
			if mode == "success" || mode == "recovered_validation_failure" {
				send("item/started", map[string]any{"threadId": threadID, "turnId": eventTurnID, "startedAtMs": 1, "item": map[string]any{"type": "imageGeneration", "id": callID, "status": "in_progress", "result": ""}})
			}
			if mode != "missing_image" && mode != "summary_only" {
				public(imageItem)
			}
			if mode != "missing_output" {
				raw(toolOutput)
			}
			if second && mode == "view_after_edit" {
				view()
			}
			if mode == "conflicting_output" {
				toolOutput["output"] = []any{map[string]any{"type": "input_image", "image_url": "data:image/png;base64," + base64.StdEncoding.EncodeToString(picture(true))}}
				raw(toolOutput)
			}
			if mode == "duplicate_terminal" {
				public(imageItem)
			}
			if mode == "conflicting_terminal" {
				altered := map[string]any{"type": "imageGeneration", "id": callID, "status": "failed", "result": ""}
				public(altered)
			}
			if mode == "early" {
				start()
			}
			if mode == "missing_turn" {
				return
			}
			if mode == "missing_image" {
				terminal["items"] = []any{}
			}
			if second && mode == "changed_original_artifact" {
				_ = os.WriteFile(firstPath, data, 0600)
			}
			turns = append(turns, terminal)
			send("turn/completed", map[string]any{"threadId": threadID, "turn": terminal})
			if mode == "conflicting_turn" {
				send("turn/completed", map[string]any{"threadId": threadID, "turn": map[string]any{"id": turnID, "status": "failed", "items": []any{}, "error": map[string]any{"message": "PRIVATE_CANARY"}}})
			}
			continue
		case "thread/read":
			if mode == "read_error" || mode == "edit_read_error" && len(turns) == 2 {
				_ = output.Encode(map[string]any{"id": request.ID, "error": map[string]any{"code": -32603, "message": "PRIVATE_CANARY"}})
				return
			}
			if mode == "late_conflict" {
				send("item/completed", map[string]any{"threadId": "private-thread", "turnId": "private-turn-1", "completedAtMs": 1, "item": map[string]any{"type": "imageGeneration", "id": "private-call-1", "status": "failed", "result": ""}})
			}
			if mode == "read_wrong_thread" {
				result = map[string]any{"thread": imageBoundThread("PRIVATE_CANARY", home, turns)}
			} else {
				if mode == "read_missing_image" {
					turns[len(turns)-1].(map[string]any)["items"] = []any{}
				}
				if mode == "read_wrong_turn" {
					turns[len(turns)-1].(map[string]any)["id"] = "PRIVATE_CANARY"
				}
				if mode == "read_failed" {
					turns[len(turns)-1].(map[string]any)["status"] = "failed"
				}
				if mode == "changed_prior" && len(turns) == 2 {
					turns[0].(map[string]any)["items"] = []any{}
				}
				history := append([]any(nil), turns...)
				if mode == "reordered_history" && len(history) == 2 {
					history[0], history[1] = history[1], history[0]
				}
				if mode == "extra_history" || mode == "extra_image_history" {
					items := []any{}
					if mode == "extra_image_history" {
						items = history[0].(map[string]any)["items"].([]any)
					}
					history = append(history, map[string]any{"id": "uncredited-history", "status": "completed", "items": items})
				}
				if mode == "duplicate_current" {
					history = append(history, history[len(history)-1])
				}
				if mode == "duplicate_prior" && len(history) == 2 {
					history = append(history, history[0])
				}
				if mode == "missing_prior" && len(history) == 2 {
					history = history[1:]
				}
				result = map[string]any{"thread": imageBoundThread("private-thread", home, history)}
			}
		default:
			return
		}
		_ = output.Encode(map[string]any{"id": request.ID, "result": result})
	}
}
