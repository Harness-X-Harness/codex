package live

import (
	"encoding/json"
	"errors"
)

// FailureInfo is a bounded diagnostic for a rejected observation. It does not
// identify a root cause or change the scenario's proof outcome.
type FailureInfo struct {
	Code           string `json:"code"`
	TerminalStatus string `json:"terminal_status,omitempty"`
	ErrorKind      string `json:"error_kind,omitempty"`
	HTTPStatus     int    `json:"http_status,omitempty"`
}

type observationFailure struct {
	message string
	info    FailureInfo
}

func (failure *observationFailure) Error() string { return failure.message }

// DescribeFailure never returns arbitrary error text or provider content.
// Unclassified errors remain unknown instead of being attributed by their text.
func DescribeFailure(err error) FailureInfo {
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
	info := FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "unknown"}
	switch status {
	case "completed", "failed", "interrupted", "inProgress":
		info.TerminalStatus = status
	}
	if len(raw) == 0 || string(raw) == "null" {
		return &observationFailure{message: message, info: info}
	}
	info.ErrorKind = "unknown"
	if len(raw) > 16<<10 {
		return &observationFailure{message: message, info: info}
	}
	var terminal struct {
		Info json.RawMessage `json:"codexErrorInfo"`
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
