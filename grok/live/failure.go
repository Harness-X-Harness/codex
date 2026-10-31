package live

import (
	"encoding/json"
	"errors"
	"strconv"
	"strings"
)

// FailureInfo is a bounded diagnostic for a rejected observation. It does not
// identify a root cause or change the scenario's proof outcome.
type FailureInfo struct {
	Code           string           `json:"code"`
	TerminalStatus string           `json:"terminal_status,omitempty"`
	ErrorKind      string           `json:"error_kind,omitempty"`
	HTTPStatus     int              `json:"http_status,omitempty"`
	HostedWeb      HostedWebFailure `json:"hosted_web,omitzero"`
}

// HostedWebFailure projects the content-free diagnostic emitted by
// codex-api/src/sse/grok.rs::invalid_web_completion. It is not replay evidence
// or an acceptance rule. Unknown formats remain absent; all fields are values
// so copying FailureInfo cannot alias caller-owned diagnostic state.
type HostedWebFailure struct {
	Reason               string `json:"reason"`
	Status               string `json:"status"`
	Action               string `json:"action"`
	ReplayBytes          string `json:"replay_bytes"` // A finite size class, not an exact byte count.
	ReplayCapBytes       uint64 `json:"replay_cap_bytes"`
	OutputIndex          uint64 `json:"output_index"`
	FrameBytes           uint64 `json:"frame_bytes"`
	CompletionStatusSeen bool   `json:"completion_status_seen"`
}

type observationFailure struct {
	message string
	info    FailureInfo
}

func (failure *observationFailure) Error() string { return failure.message }

// DescribeFailure never returns arbitrary error text or provider content.
// Nil means no failure. Unclassified errors remain unknown instead of being
// attributed by their text.
func DescribeFailure(err error) FailureInfo {
	if err == nil {
		return FailureInfo{}
	}
	var failure *observationFailure
	if errors.As(err, &failure) {
		return failure.info
	}
	return FailureInfo{Code: "unclassified_failure"}
}

func failedObservation(code, message string) error {
	return &observationFailure{message: message, info: FailureInfo{Code: code}}
}

// Called only after the caller has bound the terminal to its expected turn.
// Oversized or unfamiliar diagnostics never alter the existing rejection.
func failedTurn(status string, raw json.RawMessage, message string) error {
	info := FailureInfo{Code: "turn_unsuccessful", TerminalStatus: diagnosticTerminalStatus(status)}
	if len(raw) == 0 || string(raw) == "null" {
		return &observationFailure{message: message, info: info}
	}
	info.ErrorKind = "unknown"
	if len(raw) > 16<<10 {
		return &observationFailure{message: message, info: info}
	}
	var terminal struct {
		Info              json.RawMessage `json:"codexErrorInfo"`
		Message           json.RawMessage `json:"message"`
		AdditionalDetails json.RawMessage `json:"additionalDetails"`
	}
	if json.Unmarshal(raw, &terminal) != nil {
		return &observationFailure{message: message, info: info}
	}
	var unit string
	if json.Unmarshal(terminal.Info, &unit) == nil {
		switch unit {
		case "contextWindowExceeded", "sessionBudgetExceeded", "usageLimitExceeded",
			"rateLimitExceeded", "flexUnavailable", "serverOverloaded", "cyberPolicy",
			"misalignmentPolicyViolation", "internalServerError", "unauthorized",
			"badRequest", "threadRollbackFailed", "sandboxError", "other":
			info.ErrorKind = unit
		}
		if info.ErrorKind == "other" {
			info.HostedWeb = hostedWebDiagnostic(terminal.Message, terminal.AdditionalDetails)
		}
		return &observationFailure{message: message, info: info}
	}
	var variants map[string]json.RawMessage
	if json.Unmarshal(terminal.Info, &variants) != nil || len(variants) != 1 {
		return &observationFailure{message: message, info: info}
	}
	for kind, payload := range variants {
		switch kind {
		case "httpConnectionFailed", "responseStreamConnectionFailed",
			"responseStreamDisconnected", "responseTooManyFailedAttempts":
			var detail struct {
				Status *int `json:"httpStatusCode"`
			}
			if string(payload) != "null" && json.Unmarshal(payload, &detail) == nil {
				info.ErrorKind = kind
				if detail.Status != nil && *detail.Status >= 100 && *detail.Status <= 599 {
					info.HTTPStatus = *detail.Status
				}
				if kind == "responseStreamDisconnected" {
					info.HostedWeb = hostedWebDiagnostic(terminal.Message, terminal.AdditionalDetails)
				}
			}
		case "activeTurnNotSteerable":
			var detail struct {
				Kind string `json:"turnKind"`
			}
			if json.Unmarshal(payload, &detail) == nil && (detail.Kind == "review" || detail.Kind == "compact") {
				info.ErrorKind = kind
			}
		}
	}
	return &observationFailure{message: message, info: info}
}

func hostedWebDiagnostic(message, additionalDetails json.RawMessage) HostedWebFailure {
	var result HostedWebFailure
	for _, raw := range []json.RawMessage{message, additionalDetails} {
		var text string
		if json.Unmarshal(raw, &text) != nil {
			continue
		}
		if !strings.Contains(text, "invalid completed Web search (") {
			continue
		}
		value := parseHostedWebDiagnostic(text)
		// A malformed candidate or conflicting copies cannot establish a class.
		if value.Reason == "" || (result.Reason != "" && result != value) {
			return HostedWebFailure{}
		}
		result = value
	}
	return result
}

func parseHostedWebDiagnostic(text string) HostedWebFailure {
	// The raw runtime error and its existing App Server stream wrapper are the
	// only supported forms. Never search arbitrary error text for numbers.
	text = strings.TrimPrefix(text, "stream disconnected before completion: ")
	const prefix = "Grok stream: invalid completed Web search ("
	if !strings.HasPrefix(text, prefix) || !strings.HasSuffix(text, ")") {
		return HostedWebFailure{}
	}
	parts := strings.Split(strings.TrimSuffix(strings.TrimPrefix(text, prefix), ")"), ", ")
	keys := [...]string{"reason=", "status=", "action=", "replay_bytes=", "replay_cap_bytes=", "output_index=", "frame_bytes=", "completion_status_seen="}
	if len(parts) != len(keys) {
		return HostedWebFailure{}
	}
	for index, key := range keys {
		if !strings.HasPrefix(parts[index], key) {
			return HostedWebFailure{}
		}
		parts[index] = strings.TrimPrefix(parts[index], key)
	}
	result := HostedWebFailure{Reason: parts[0], Status: parts[1], Action: parts[2], ReplayBytes: parts[3]}
	if !result.knownLabels() {
		return HostedWebFailure{}
	}
	for index, target := range []*uint64{&result.ReplayCapBytes, &result.OutputIndex, &result.FrameBytes} {
		value, err := strconv.ParseUint(parts[index+4], 10, 64)
		if err != nil || strconv.FormatUint(value, 10) != parts[index+4] {
			return HostedWebFailure{}
		}
		*target = value
	}
	switch parts[7] {
	case "true":
		result.CompletionStatusSeen = true
	case "false":
	default:
		return HostedWebFailure{}
	}
	return result
}

func (failure HostedWebFailure) knownLabels() bool {
	switch failure.Reason {
	case "unsupported_item", "missing_identity", "missing_status", "non_completed_status",
		"missing_action", "unknown_action", "replay_too_large", "invalid_projection":
	default:
		return false
	}
	switch failure.Status {
	case "missing", "completed", "failed", "incomplete", "in_progress", "searching", "cancelled", "other", "not_web":
	default:
		return false
	}
	switch failure.Action {
	case "missing", "search", "open_page", "find_in_page", "other", "not_web":
	default:
		return false
	}
	switch failure.ReplayBytes {
	case "not_measured", "over_cap", "within_cap":
		return true
	default:
		return false
	}
}

func diagnosticTerminalStatus(status string) string {
	switch status {
	case "completed", "failed", "interrupted", "inProgress":
		return status
	default:
		return "unknown"
	}
}
