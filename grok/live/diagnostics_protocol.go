package live

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// protocolDiagnostic projects public task events, not raw transport or history.
// Reasoning notifications are deliberately represented only by their existence.
func protocolDiagnostic(d *Diagnostics, source string, value any) {
	if d == nil {
		return
	}
	raw, err := json.Marshal(value)
	if err != nil {
		d.Record("capture_unavailable", map[string]any{"source": source, "reason": "public_event_encoding"})
		return
	}
	var envelope struct {
		Method string                     `json:"method"`
		Params map[string]json.RawMessage `json:"params"`
	}
	if json.Unmarshal(raw, &envelope) != nil {
		return
	}
	method := envelope.Method
	fields := map[string]any{"source": source, "method": method}
	for _, key := range []string{"threadId", "turnId", "itemId", "callId"} {
		if v := envelope.Params[key]; len(v) > 0 {
			fields[key] = v
		}
	}
	if strings.Contains(strings.ToLower(method), "reasoning") {
		fields["omitted"] = "private_reasoning"
		d.Record("public_event", fields)
		return
	}
	if item := envelope.Params["item"]; len(item) > 0 {
		d.Item(method, item)
	}
	if turn := envelope.Params["turn"]; len(turn) > 0 {
		var t struct {
			ID     string            `json:"id"`
			Status string            `json:"status"`
			Error  json.RawMessage   `json:"error"`
			Items  []json.RawMessage `json:"items"`
		}
		if json.Unmarshal(turn, &t) == nil {
			fields["turnId"], fields["status"] = t.ID, t.Status
			if len(t.Error) > 0 {
				fields["error"] = t.Error
			}
			for _, item := range t.Items {
				d.Item(method, item)
			}
		}
	}
	// Allow only known public task payloads. Account/auth/config/environment
	// notifications never contribute their payload to the bundle.
	switch method {
	case "item/agentMessage/delta", "item/commandExecution/outputDelta", "item/fileChange/outputDelta":
		fields["delta"] = envelope.Params["delta"]
	case "item/tool/call", "item/commandExecution/requestApproval", "item/fileChange/requestApproval", "item/tool/requestUserInput", "execCommandApproval", "applyPatchApproval":
		for _, key := range []string{"tool", "arguments", "command", "reason", "changes", "questions", "grantRoot", "cwd"} {
			if v := envelope.Params[key]; len(v) > 0 {
				fields[key] = v
			}
		}
	case "error":
		if v := envelope.Params["error"]; len(v) > 0 {
			fields["error"] = v
		}
		if v := envelope.Params["willRetry"]; len(v) > 0 {
			fields["willRetry"] = v
		}
	}
	d.Record("public_event", fields)
}

func diagnosticFailure(err error) any {
	if err == nil {
		return nil
	}
	return DescribeFailure(err)
}

// Preserve public task output before validating it; never serialize raw history or reasoning.
func recordProductDiagnosticTurn(diagnostics *Diagnostics, source, threadID string, turn productTurn) {
	if diagnostics == nil {
		return
	}
	fields := map[string]any{"thread_id": threadID, "turn_id": turn.ID, "status": turn.Status}
	if len(turn.Error) != 0 {
		fields["error"] = turn.Error
	}
	if turn.Status == "failed" || turn.Status == "interrupted" {
		if err := completedProductTurn(turn); err != nil {
			fields["failure"] = DescribeFailure(err)
		}
	}
	diagnostics.Record(source, fields)
	for _, item := range turn.Items {
		if _, reasoning := item.AsReasoning(); reasoning {
			continue
		}
		raw, err := json.Marshal(item)
		if err == nil {
			diagnostics.Item(source, raw)
		}
	}
}

// Observe decoded public snapshots before oracle validation can reject them.
func recordSDKDiagnosticTurn(diagnostics *Diagnostics, source, threadID string, value protocolv2.Turn) {
	if diagnostics == nil {
		return
	}
	turn := productTurn{ID: value.ID, Status: string(value.Status), Items: value.Items}
	if value.Error != nil && value.Error.Value != nil {
		turn.Error, _ = json.Marshal(*value.Error.Value)
	}
	recordProductDiagnosticTurn(diagnostics, source, threadID, turn)
}

// Diagnostics retain every public image attempt, not just the final image
// credited by the oracle. Capture never changes acceptance or reads saved paths.
func recordImageTurn(diagnostics *Diagnostics, phase string, turn productTurn) {
	if diagnostics == nil {
		return
	}
	for index, item := range turn.Items {
		image, ok := item.AsImageGeneration()
		if !ok {
			continue
		}
		fields := map[string]any{"phase": phase, "item": index, "status": image.Status,
			"failed": image.Failure != nil && image.Failure.Value != nil}
		if image.Result != "" {
			data, err := imagePayload(image.Result)
			if err != nil {
				fields["capture"] = "invalid_png"
			} else {
				fields["artifact"] = diagnostics.Artifact(fmt.Sprintf("%s-%d.png", phase, index), data)
				fields["sha256"] = editDigest(data)
			}
		} else {
			fields["capture"] = "no_image_payload"
		}
		diagnostics.Record("image_attempt", fields)
	}
}

func (stream *childStream) recordDiagnostic(message frame) {
	if stream.diagnostics == nil || message.Method != "rawResponseItem/completed" {
		return
	}
	var event childRawEvent
	if json.Unmarshal(message.Params, &event) != nil {
		return
	}
	if event.Item.Type != "agent_message" && event.Item.Type != "message" {
		return
	}
	var text []string
	for _, part := range event.Item.Content {
		if part.Type == "input_text" || part.Type == "output_text" {
			text = append(text, part.Text)
		}
	}
	stream.diagnostics.Record("child_visible_message", map[string]any{"thread_id": event.ThreadID, "turn_id": event.TurnID, "item_id": event.Item.ID, "type": event.Item.Type, "role": event.Item.Role, "phase": event.Item.Phase, "author": event.Item.Author, "recipient": event.Item.Recipient, "text": text})
}
