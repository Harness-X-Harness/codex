package live

import (
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"testing"
)

func TestFailureDiagnosticsExcludePrivateContent(t *testing.T) {
	cases := []struct {
		name, status, raw string
		want              FailureInfo
	}{
		{"typed", "failed", `{"message":"PRIVATE_CANARY","additionalDetails":"PRIVATE_CANARY","codexErrorInfo":{"responseStreamDisconnected":{"httpStatusCode":429,"private":"PRIVATE_CANARY"}}}`, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "responseStreamDisconnected", HTTPStatus: 429}},
		{"unit", "failed", `{"codexErrorInfo":"badRequest","misalignment":{"detailedExplanation":"PRIVATE_CANARY"}}`, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "badRequest"}},
		{"interrupted", "interrupted", "null", FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "interrupted"}},
		{"completed_error", "completed", `{"codexErrorInfo":"unauthorized"}`, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "completed", ErrorKind: "unauthorized"}},
		{"unknown_status", "PRIVATE_CANARY", "", FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "unknown"}},
		{"unknown_variant", "failed", `{"codexErrorInfo":"PRIVATE_CANARY"}`, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "unknown"}},
		{"unknown_object", "failed", `{"codexErrorInfo":{"PRIVATE_CANARY":{"httpStatusCode":429}}}`, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "unknown"}},
		{"malformed", "failed", "PRIVATE_CANARY", FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "unknown"}},
		{"oversized", "failed", strings.Repeat("x", 16385), FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "unknown"}},
		{"multiple_variants", "failed", `{"codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":401},"responseStreamDisconnected":{"httpStatusCode":429}}}`, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "unknown"}},
		{"malformed_variant", "failed", `{"codexErrorInfo":{"httpConnectionFailed":"PRIVATE_CANARY"}}`, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "unknown"}},
		{"string_status", "failed", `{"codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":"PRIVATE_CANARY"}}}`, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "unknown"}},
		{"fractional_status", "failed", `{"codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":429.5}}}`, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "unknown"}},
		{"out_of_range", "failed", `{"codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":600}}}`, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "httpConnectionFailed"}},
		{"no_status", "failed", `{"codexErrorInfo":{"responseTooManyFailedAttempts":{"httpStatusCode":null}}}`, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "responseTooManyFailedAttempts"}},
		{"ineligible_status", "failed", `{"codexErrorInfo":"badRequest","httpStatusCode":401}`, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "badRequest"}},
		{"active_turn", "failed", `{"codexErrorInfo":{"activeTurnNotSteerable":{"turnKind":"review"}}}`, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "activeTurnNotSteerable"}},
		{"unknown_turn_kind", "failed", `{"codexErrorInfo":{"activeTurnNotSteerable":{"turnKind":"PRIVATE_CANARY"}}}`, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "unknown"}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			err := failedTurn(tc.status, json.RawMessage(tc.raw), "live: turn did not complete successfully")
			if err.Error() != "live: turn did not complete successfully" {
				t.Fatal("diagnostics changed the existing static error")
			}
			got := DescribeFailure(fmt.Errorf("PRIVATE_CANARY: %w", err))
			if !reflect.DeepEqual(got, tc.want) {
				t.Fatalf("diagnostic = %+v, want %+v", got, tc.want)
			}
			encoded, err := json.Marshal(got)
			if err != nil || strings.Contains(string(encoded), "PRIVATE_CANARY") {
				t.Fatal("diagnostic retained private content")
			}
		})
	}
}

func TestFailureDiagnosticsPreserveUnknownErrors(t *testing.T) {
	for _, err := range []error{nil, errors.New("PRIVATE_CANARY"), errors.New("live: unsupported server request")} {
		if got := DescribeFailure(err); got != (FailureInfo{Code: "unclassified_failure"}) {
			t.Fatalf("untyped error was attributed: %+v", got)
		}
	}
	original := failedObservation("unsupported_server_request", "live: unsupported server request")
	if original.Error() != "live: unsupported server request" || DescribeFailure(original) != (FailureInfo{Code: "unsupported_server_request"}) {
		t.Fatal("typed observation did not preserve its static error and code")
	}
}
