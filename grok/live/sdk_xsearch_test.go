package live

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/ronhuafeng/llm-go/codexsdk"
)

const controlledXDecode = `codexsdk: decode item/started notification: decode ServerNotification.params: decode ItemStartedNotification.item: decode ThreadItem.type: unknown variant "xSearch"`

func TestSDKXSearchClassificationRequiresJoinedPublicCause(t *testing.T) {
	xerr := errors.New(controlledXDecode)
	for _, tc := range []struct {
		name              string
		cause, close, run error
		reaped            bool
		pending           int64
		cancel            bool
		want              bool
	}{
		{"masked_transport", xerr, xerr, errSDKProtocolEnded, true, 0, false, true},
		{"no_run_error", xerr, xerr, nil, true, 0, false, true},
		{"ordinary_business", xerr, xerr, errors.New("business failure"), true, 0, false, false},
		{"generic_string_not_provenance", xerr, xerr, errors.New(errSDKProtocolEnded.Error()), true, 0, false, false},
		{"rpc_impersonation", &codexsdk.ProtocolError{Message: controlledXDecode, Err: xerr}, xerr, nil, true, 0, false, false},
		{"shutdown_timeout", xerr, &codexsdk.ShutdownError{Result: codexsdk.ShutdownResult{ProcessReaped: true, Cause: xerr}, Err: context.DeadlineExceeded}, nil, true, 0, false, false},
		{"missing_cause", nil, xerr, errSDKProtocolEnded, true, 0, false, false},
		{"missing_close", xerr, nil, errSDKProtocolEnded, true, 0, false, false},
		{"other_variant", errors.New(strings.ReplaceAll(controlledXDecode, "xSearch", "PRIVATE_OTHER")), xerr, nil, true, 0, false, false},
		{"decorated", errors.New(controlledXDecode + " PRIVATE_TEXT"), xerr, nil, true, 0, false, false},
		{"timeout", xerr, context.DeadlineExceeded, nil, true, 0, false, false},
		{"joined_failure", xerr, errors.Join(xerr, errors.New("cleanup failed")), nil, true, 0, false, false},
		{"not_reaped", xerr, xerr, nil, false, 0, false, false},
		{"pending_callbacks", xerr, xerr, nil, true, 1, false, false},
		{"expired_scenario", xerr, xerr, nil, true, 0, true, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			if tc.cancel {
				cancel()
			}
			closed := codexsdk.ShutdownResult{Cause: tc.cause, ProcessReaped: tc.reaped, CallbackWorkPending: tc.pending}
			if got := sdkRejectedXSearch(ctx, closed, tc.close, tc.run); got != tc.want {
				t.Fatalf("classification=%t want=%t", got, tc.want)
			}
		})
	}
}

func TestAcceptedXSearchRejectionRequiresUnmixedFinalEvidence(t *testing.T) {
	valid := SearchEvidence{Scenario: "x", ExpectedUnsupported: true, Evidence: Evidence{Bound: true, Processes: 1, Initializations: 1, Threads: 1, Turns: 1, Stage: "search_turn_submitted"}}
	if !AcceptedXSearchRejection(valid, errSDKXSearchUnsupported) {
		t.Fatal("controlled expected rejection absent")
	}
	for _, tc := range []struct {
		name   string
		mutate func(*SearchEvidence)
		err    error
	}{
		{"artifact_changed", func(e *SearchEvidence) {}, errors.Join(errSDKXSearchUnsupported, errors.New("artifact changed"))},
		{"generic_end", func(e *SearchEvidence) {}, errSDKProtocolEnded},
		{"no_error", func(e *SearchEvidence) {}, nil},
		{"missing_marker", func(e *SearchEvidence) { e.ExpectedUnsupported = false }, errSDKXSearchUnsupported},
		{"web", func(e *SearchEvidence) { e.Scenario = "web" }, errSDKXSearchUnsupported},
		{"cleanup", func(e *SearchEvidence) { e.CleanupPending = true }, errSDKXSearchUnsupported},
		{"completed", func(e *SearchEvidence) { e.Completed = true }, errSDKXSearchUnsupported},
		{"first_completed", func(e *SearchEvidence) { e.FirstCompleted = true }, errSDKXSearchUnsupported},
		{"followup", func(e *SearchEvidence) { e.Turns = 2 }, errSDKXSearchUnsupported},
		{"settled", func(e *SearchEvidence) { e.SettledTurns = 1 }, errSDKXSearchUnsupported},
		{"unbound", func(e *SearchEvidence) { e.Bound = false }, errSDKXSearchUnsupported},
		{"preflight", func(e *SearchEvidence) { e.Stage = "preflight" }, errSDKXSearchUnsupported},
	} {
		t.Run(tc.name, func(t *testing.T) {
			e := valid
			tc.mutate(&e)
			if AcceptedXSearchRejection(e, tc.err) {
				t.Fatal("invalid expected rejection accepted")
			}
		})
	}
}

func TestSearchRejectionRevalidatesObservedRawWithoutNewCredit(t *testing.T) {
	for _, tc := range []struct {
		name      string
		items     []string
		wantError bool
	}{
		{"observed_local", []string{`{"type":"custom_tool_call","id":"local","name":"apply_patch","input":"PRIVATE_INPUT"}`}, true},
		{"canonical_uncredited", []string{`{"type":"custom_tool_call","id":"call","call_id":"call","status":"completed","name":"x_keyword_search","input":"PRIVATE_INPUT"}`}, false},
		{"canonical_conflict", []string{`{"type":"custom_tool_call","id":"call","call_id":"call","status":"completed","name":"x_keyword_search","input":"PRIVATE_INPUT"}`, `{"type":"custom_tool_call","id":"call","call_id":"call","status":"completed","name":"x_keyword_search","input":"PRIVATE_CHANGED"}`}, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			evidence := Evidence{Stage: "search_turn_submitted"}
			result := SearchEvidence{Scenario: "x"}
			transport := &sdkProductObserver{}
			for _, item := range tc.items {
				params := []byte(`{"threadId":"thread","turnId":"turn","item":` + item + `}`)
				transport.pending = append(transport.pending, frame{Method: "rawResponseItem/completed", Params: params, size: len(params)})
			}
			probe := searchProbe{server: transport, threadID: "thread", turnID: "turn", scenario: "x", evidence: &evidence, result: &result, calls: map[string]map[string]string{"turn": {}}}
			err := probe.revalidate(nil)
			if (err != nil) != tc.wantError {
				t.Fatalf("validation=%v", err)
			}
			if result.CanonicalCalls != 0 || result.SettledTurns != 0 || evidence.Stage != "search_turn_submitted" || len(probe.calls["turn"]) != 0 {
				t.Fatal("cleanup created business credit")
			}
		})
	}
}
