package live

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"time"
	"unicode/utf8"
)

const diagnosticsDeltaSamples = 16

// Samples counts admitted trace records, not successful filesystem writes.
// Events and Bytes include deliberately unsampled deltas; write failures remain
// in the ordinary capture counters. Only the three public output methods enter
// this map, so its size does not grow with turns, items or provider identifiers.
type DiagnosticDelta struct {
	Events  int `json:"events"`
	Bytes   int `json:"bytes"`
	Samples int `json:"samples"`
}

type DiagnosticTerminal struct {
	At             string      `json:"at"`
	ThreadIDSHA256 string      `json:"thread_id_sha256,omitempty"`
	TurnIDSHA256   string      `json:"turn_id_sha256,omitempty"`
	Status         string      `json:"status"`
	Failure        FailureInfo `json:"failure,omitzero"`
}

type DiagnosticProtocolError struct {
	At             string      `json:"at"`
	ThreadIDSHA256 string      `json:"thread_id_sha256,omitempty"`
	TurnIDSHA256   string      `json:"turn_id_sha256,omitempty"`
	WillRetry      *bool       `json:"will_retry,omitempty"`
	Failure        FailureInfo `json:"failure"`
}

type DiagnosticResult struct {
	At             string      `json:"at"`
	Stage          string      `json:"stage"`
	Completed      bool        `json:"completed"`
	CleanupPending bool        `json:"cleanup_pending"`
	Failure        FailureInfo `json:"failure,omitzero"`
}

type DiagnosticCleanup struct {
	At                  string      `json:"at"`
	ProcessReaped       bool        `json:"process_reaped"`
	CallbackWorkPending int64       `json:"callback_work_pending"`
	Failed              bool        `json:"failed"`
	Failure             FailureInfo `json:"failure,omitzero"`
}

type DiagnosticInvocation struct {
	At       string      `json:"at"`
	Outcome  string      `json:"outcome"`
	Reason   string      `json:"reason,omitempty"`
	ExitCode int         `json:"exit_code"`
	Failure  FailureInfo `json:"failure,omitzero"`
}

// Record already holds mu. Retain only fixed, typed facts, independently of
// event admission. Protocol completion, fixture evidence and cleanup are
// separate facts; only the caller's invocation outcome states its verdict.
// No arbitrary error text is copied.
func (d *Diagnostics) retainSummary(kind string, fields map[string]any) {
	method, _ := fields["method"].(string)
	if kind != "fixture_result" && kind != "sdk_cleanup" && kind != "invocation_finished" &&
		(kind != "public_event" || (method != "turn/completed" && method != "error")) {
		return
	}
	at := time.Now().UTC().Format(time.RFC3339Nano)
	switch kind {
	case "fixture_result":
		completed, _ := fields["completed"].(bool)
		pending, _ := fields["cleanup_pending"].(bool)
		d.status.Result = &DiagnosticResult{At: at, Stage: d.summaryText(fields["stage"]), Completed: completed, CleanupPending: pending, Failure: d.summaryFailure(fields["failure"])}
	case "sdk_cleanup":
		reaped, _ := fields["process_reaped"].(bool)
		pending, _ := fields["callback_work_pending"].(int64)
		failed, _ := fields["failed"].(bool)
		d.status.Cleanup = &DiagnosticCleanup{At: at, ProcessReaped: reaped, CallbackWorkPending: pending, Failed: failed, Failure: d.summaryFailure(fields["failure"])}
	case "invocation_finished":
		code, _ := fields["exit_code"].(int)
		d.status.Invocation = &DiagnosticInvocation{At: at, Outcome: d.summaryText(fields["outcome"]), Reason: d.summaryText(fields["reason"]), ExitCode: code, Failure: d.summaryFailure(fields["failure"])}
	case "public_event":
		raw, _ := fields["error"].(json.RawMessage)
		switch method {
		case "turn/completed":
			status, _ := fields["status"].(string)
			status = diagnosticTerminalStatus(status)
			var failure FailureInfo
			if status != "completed" || (len(raw) > 0 && string(raw) != "null") {
				failure = DescribeFailure(failedTurn(status, raw, ""))
			}
			d.status.LastTerminal = &DiagnosticTerminal{At: at, ThreadIDSHA256: diagnosticIdentity(fields["threadId"]), TurnIDSHA256: diagnosticIdentity(fields["turnId"]), Status: status, Failure: failure}
		case "error":
			failure := DescribeFailure(failedTurn("", raw, ""))
			failure.Code, failure.TerminalStatus = "protocol_error", ""
			var retry *bool
			if value, ok := fields["willRetry"].(json.RawMessage); ok {
				if json.Unmarshal(value, &retry) != nil {
					retry = nil
				}
			}
			d.status.LastError = &DiagnosticProtocolError{At: at, ThreadIDSHA256: diagnosticIdentity(fields["threadId"]), TurnIDSHA256: diagnosticIdentity(fields["turnId"]), WillRetry: retry, Failure: failure}
		}
	}
}

// Status is also embedded in the public command Report, which permits hashes
// but not raw protocol identities. Full correlations stay in auxiliary events.
func diagnosticIdentity(value any) string {
	var identity string
	switch value := value.(type) {
	case string:
		identity = value
	case json.RawMessage:
		_ = json.Unmarshal(value, &identity)
	}
	if identity == "" {
		return ""
	}
	digest := sha256.Sum256([]byte(identity))
	return hex.EncodeToString(digest[:])
}

func (d *Diagnostics) summaryText(value any) string {
	var text string
	switch value := value.(type) {
	case string:
		text = value
	case json.RawMessage:
		_ = json.Unmarshal(value, &text)
	}
	text = d.cleanString(text)
	if len(text) > 1024 {
		d.limit("summary_field_limit")
		text = text[:1024]
		for !utf8.ValidString(text) {
			text = text[:len(text)-1]
		}
		text += "[TRUNCATED]"
	}
	return text
}

func (d *Diagnostics) summaryFailure(value any) FailureInfo {
	var result FailureInfo
	switch value := value.(type) {
	case FailureInfo:
		result = value
	case *FailureInfo:
		if value != nil {
			result = *value
		}
	}
	result.Code = d.summaryText(result.Code)
	result.TerminalStatus = d.summaryText(result.TerminalStatus)
	result.ErrorKind = d.summaryText(result.ErrorKind)
	if !result.HostedWeb.knownLabels() {
		result.HostedWeb = HostedWebFailure{}
	}
	return result
}

func (d *Diagnostics) countDelta(kind string, fields map[string]any) (string, int) {
	if kind != "public_event" {
		return "", 0
	}
	method, _ := fields["method"].(string)
	switch method {
	case "item/agentMessage/delta", "item/commandExecution/outputDelta", "item/fileChange/outputDelta":
	default:
		return "", 0
	}
	if d.status.Deltas == nil {
		d.status.Deltas = make(map[string]DiagnosticDelta, 3)
	}
	var delta string
	switch value := fields["delta"].(type) {
	case string:
		delta = value
	case json.RawMessage:
		_ = json.Unmarshal(value, &delta)
	}
	counts := d.status.Deltas[method]
	incrementDiagnosticCount(&counts.Events)
	counts.Bytes += min(len(delta), int(^uint(0)>>1)-counts.Bytes)
	d.status.Deltas[method] = counts
	return method, len(delta)
}
