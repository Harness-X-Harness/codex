package live

import (
	"encoding/json"
	"strings"
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
	return DescribeFailure(sdkFailure(err))
}
