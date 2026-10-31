package live

import (
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"
)

const failedWebDiagnostic = "Grok stream: invalid completed Web search (reason=non_completed_status, status=failed, action=open_page, replay_bytes=not_measured, replay_cap_bytes=40000, output_index=1, frame_bytes=279, completion_status_seen=false)"

func webDiagnosticError(t *testing.T, info, message, additional any) json.RawMessage {
	t.Helper()
	raw, err := json.Marshal(map[string]any{"codexErrorInfo": info, "message": message, "additionalDetails": additional})
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

func TestFailureHostedWebDiagnosticUsesOwnedFiniteFormat(t *testing.T) {
	want := HostedWebFailure{Reason: "non_completed_status", Status: "failed", Action: "open_page", ReplayBytes: "not_measured", ReplayCapBytes: 40000, OutputIndex: 1, FrameBytes: 279}
	wrapped := "stream disconnected before completion: " + failedWebDiagnostic
	for _, test := range []struct {
		name, kind                string
		info, message, additional any
	}{
		{"runtime", "other", "other", failedWebDiagnostic, nil},
		{"terminal", "other", "other", wrapped, nil},
		{"retry", "responseStreamDisconnected", map[string]any{"responseStreamDisconnected": map[string]any{"httpStatusCode": nil}}, "Reconnecting...", wrapped},
		{"matching_copies", "other", "other", wrapped, failedWebDiagnostic},
	} {
		t.Run(test.name, func(t *testing.T) {
			err := failedTurn("failed", webDiagnosticError(t, test.info, test.message, test.additional), "live: turn did not complete successfully")
			got := DescribeFailure(err)
			if err == nil || err.Error() != "live: turn did not complete successfully" || got.Code != "turn_unsuccessful" || got.TerminalStatus != "failed" || got.ErrorKind != test.kind || got.HostedWeb != want {
				t.Fatalf("classification changed failure semantics or lost finite fields: %+v", got)
			}
		})
	}
	// These are formatter labels, not a new validity matrix for provider items.
	for _, value := range []HostedWebFailure{
		{Reason: "missing_identity", Status: "completed", Action: "search", ReplayBytes: "not_measured"},
		{Reason: "missing_status", Status: "missing", Action: "find_in_page", ReplayBytes: "not_measured"},
		{Reason: "missing_action", Status: "incomplete", Action: "missing", ReplayBytes: "not_measured"},
		{Reason: "unknown_action", Status: "in_progress", Action: "other", ReplayBytes: "not_measured"},
		{Reason: "unsupported_item", Status: "not_web", Action: "not_web", ReplayBytes: "not_measured"},
		{Reason: "replay_too_large", Status: "searching", Action: "search", ReplayBytes: "over_cap"},
		{Reason: "invalid_projection", Status: "cancelled", Action: "open_page", ReplayBytes: "within_cap"},
		{Reason: "non_completed_status", Status: "other", Action: "find_in_page", ReplayBytes: "not_measured"},
	} {
		value.ReplayCapBytes, value.OutputIndex, value.FrameBytes, value.CompletionStatusSeen = 40000, 0, 123, true
		text := fmt.Sprintf("Grok stream: invalid completed Web search (reason=%s, status=%s, action=%s, replay_bytes=%s, replay_cap_bytes=%d, output_index=%d, frame_bytes=%d, completion_status_seen=%t)", value.Reason, value.Status, value.Action, value.ReplayBytes, value.ReplayCapBytes, value.OutputIndex, value.FrameBytes, value.CompletionStatusSeen)
		if got := parseHostedWebDiagnostic(text); got != value {
			t.Fatalf("known formatter fields not retained: %+v", got)
		}
	}
}

func TestFailureHostedWebDiagnosticRejectsUnknownOrMalformedText(t *testing.T) {
	for name, text := range map[string]string{
		"private_reason":      strings.Replace(failedWebDiagnostic, "non_completed_status", "PRIVATE_CANARY", 1),
		"private_status":      strings.Replace(failedWebDiagnostic, "status=failed", "status=PRIVATE_CANARY", 1),
		"private_action":      strings.Replace(failedWebDiagnostic, "action=open_page", "action=PRIVATE_CANARY", 1),
		"private_replay":      strings.Replace(failedWebDiagnostic, "not_measured", "PRIVATE_CANARY", 1),
		"private_size":        strings.Replace(failedWebDiagnostic, "frame_bytes=279", "frame_bytes=PRIVATE_CANARY", 1),
		"private_prefix":      "PRIVATE_CANARY " + failedWebDiagnostic,
		"private_suffix":      failedWebDiagnostic + " PRIVATE_CANARY",
		"unknown_reason":      strings.Replace(failedWebDiagnostic, "non_completed_status", "future_reason", 1),
		"missing_close":       strings.TrimSuffix(failedWebDiagnostic, ")"),
		"missing_field":       strings.Replace(failedWebDiagnostic, ", frame_bytes=279", "", 1),
		"duplicate_field":     strings.Replace(failedWebDiagnostic, "frame_bytes=279", "frame_bytes=279, frame_bytes=279", 1),
		"negative_size":       strings.Replace(failedWebDiagnostic, "frame_bytes=279", "frame_bytes=-279", 1),
		"fractional_size":     strings.Replace(failedWebDiagnostic, "frame_bytes=279", "frame_bytes=279.5", 1),
		"noncanonical_number": strings.Replace(failedWebDiagnostic, "frame_bytes=279", "frame_bytes=0279", 1),
		"overflow":            strings.Replace(failedWebDiagnostic, "frame_bytes=279", "frame_bytes=18446744073709551616", 1),
		"noncanonical_bool":   strings.Replace(failedWebDiagnostic, "seen=false", "seen=0", 1),
		"arbitrary":           "PRIVATE_CANARY frame_bytes=279",
	} {
		t.Run(name, func(t *testing.T) {
			got := DescribeFailure(failedTurn("failed", webDiagnosticError(t, "other", text, nil), "live: rejected"))
			if got != (FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "other"}) {
				t.Fatalf("unrecognized text was attributed: %+v", got)
			}
			encoded, err := json.Marshal(got)
			if err != nil || strings.Contains(string(encoded), "PRIVATE_CANARY") {
				t.Fatal("arbitrary error content entered compact diagnostics")
			}
		})
	}
	for _, additional := range []string{
		strings.Replace(failedWebDiagnostic, "action=open_page", "action=search", 1),
		strings.Replace(failedWebDiagnostic, "frame_bytes=279", "frame_bytes=280", 1),
		strings.Replace(failedWebDiagnostic, "status=failed", "status=PRIVATE_CANARY", 1),
	} {
		got := DescribeFailure(failedTurn("failed", webDiagnosticError(t, "other", failedWebDiagnostic, additional), "live: rejected"))
		if got.HostedWeb != (HostedWebFailure{}) {
			t.Fatal("conflicting or malformed diagnostic copies were accepted")
		}
	}
	for _, raw := range []json.RawMessage{
		webDiagnosticError(t, "future_kind", failedWebDiagnostic, nil),
		webDiagnosticError(t, map[string]any{"responseStreamDisconnected": "PRIVATE_CANARY"}, failedWebDiagnostic, nil),
		webDiagnosticError(t, "other", map[string]any{"secret": "PRIVATE_CANARY"}, nil),
		webDiagnosticError(t, "other", failedWebDiagnostic+strings.Repeat(" ", 16<<10), nil),
	} {
		got := DescribeFailure(failedTurn("failed", raw, "live: rejected"))
		if got.HostedWeb != (HostedWebFailure{}) {
			t.Fatal("unrecognized envelope yielded hosted-web detail")
		}
	}
	if got := DescribeFailure(errors.New(failedWebDiagnostic)); got != (FailureInfo{Code: "unclassified_failure"}) {
		t.Fatal("arbitrary Go error strings became a classification oracle")
	}
}
