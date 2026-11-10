package live

import (
	"bytes"
	"encoding/json"
)

// The caller already encoded this envelope for its existing SDK projection.
// Bound optional parsing before inspecting it; never retain the envelope.
func recordResponseToolEnvelopeDiagnostic(d *Diagnostics, source string, raw []byte) {
	if d == nil {
		return
	}
	if len(raw) > 64<<10 {
		d.Record("response_tool_shape", map[string]any{"source": source, "envelope_bytes": len(raw), "capture": "envelope_size_limit"})
		return
	}
	var envelope struct {
		Params struct {
			ThreadID, TurnID string
			Item             json.RawMessage
		}
	}
	if json.Unmarshal(raw, &envelope) != nil {
		d.Record("response_tool_shape", map[string]any{"source": source, "capture": "envelope_json_invalid"})
		return
	}
	recordResponseToolDiagnostic(d, source, envelope.Params.ThreadID, envelope.Params.TurnID, envelope.Params.Item)
}

// This optional projection records an emitted function call before a public
// Started event can exist. It is not proof of dispatch, handler success, child
// creation or the tools actually sent to the provider. It never returns a verdict.
func recordResponseToolDiagnostic(d *Diagnostics, source, threadID, turnID string, raw json.RawMessage) {
	if d == nil {
		return
	}
	fields := map[string]any{"source": source, "thread_id": threadID, "turn_id": turnID, "item_bytes": len(raw)}
	if len(raw) > 64<<10 {
		fields["capture"] = "item_size_limit"
		d.Record("response_tool_shape", fields)
		return
	}
	var item struct {
		Type, ID, Name, Status string
		Arguments, Output      json.RawMessage
	}
	// call_id is a wire spelling, not the Go field-name convention.
	var call struct {
		CallID string `json:"call_id"`
	}
	if json.Unmarshal(raw, &item) != nil {
		fields["capture"] = "item_json_invalid"
		d.Record("response_tool_shape", fields)
		return
	}
	if item.Type != "function_call" && item.Type != "function_call_output" {
		return // Never project reasoning or unrelated payloads through this seam.
	}
	_ = json.Unmarshal(raw, &call)
	fields["type"], fields["item_id"], fields["call_id"], fields["name"] = item.Type, item.ID, call.CallID, item.Name
	fields["status"] = diagnosticToolStatus(item.Status)
	if item.Type == "function_call" {
		fields["arguments"] = diagnosticJSONShape(item.Arguments)
	} else {
		fields["output"] = diagnosticJSONShape(item.Output)
	}
	d.Record("response_tool_shape", fields)
}

func diagnosticToolStatus(status string) string {
	switch status {
	case "", "completed", "failed", "in_progress", "incomplete", "cancelled":
		return status
	default:
		return "other"
	}
}

// Values and unknown field names are never copied. Plain-text tool output is
// legitimate: json_valid=false is a diagnostic fact, not a protocol rejection.
func diagnosticJSONShape(raw json.RawMessage) map[string]any {
	shape := map[string]any{"bytes": len(raw)}
	if len(raw) > 16<<10 {
		shape["capture"] = "field_size_limit"
		return shape
	}
	if len(raw) == 0 {
		shape["kind"] = "absent"
		return shape
	}
	trimmed := bytes.TrimSpace(raw)
	if len(trimmed) > 0 && trimmed[0] == '"' {
		var text string
		if json.Unmarshal(trimmed, &text) == nil {
			raw = json.RawMessage(text)
		}
	}
	var value any
	err := json.Unmarshal(raw, &value)
	shape["json_valid"] = err == nil
	if err != nil {
		shape["kind"] = "text"
		return shape
	}
	shape["kind"] = diagnosticJSONKind(value)
	if object, ok := value.(map[string]any); ok {
		shape["field_count"] = len(object)
		known := map[string]string{}
		for _, key := range []string{"agent_id", "id", "message", "prompt", "name", "fork_turns", "target", "input", "success", "error", "status"} {
			if value, present := object[key]; present {
				known[key] = diagnosticJSONKind(value)
			}
		}
		shape["known_field_types"] = known
		if success, ok := object["success"].(bool); ok {
			shape["success"] = success
		}
		if value, present := object["error"]; present {
			shape["error_present"] = value != nil
		}
	}
	return shape
}

func diagnosticJSONKind(value any) string {
	switch value.(type) {
	case nil:
		return "null"
	case map[string]any:
		return "object"
	case []any:
		return "array"
	case string:
		return "string"
	case bool:
		return "boolean"
	case float64:
		return "number"
	default:
		return "other"
	}
}
