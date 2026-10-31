package live

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func TestDiagnosticsDeltaSamplingPreservesCountsAndTerminal(t *testing.T) {
	dir := t.TempDir()
	d := NewDiagnostics(dir, "SECRET_CANARY")
	t.Cleanup(d.Close)
	const count = 9000
	for i := 0; i < count; i++ {
		protocolDiagnostic(d, "notification", map[string]any{
			"method": "item/agentMessage/delta",
			"params": map[string]any{"threadId": "thread", "turnId": "turn", "delta": "汉 SECRET_CANARY"},
		})
	}
	protocolDiagnostic(d, "notification", map[string]any{
		"method": "turn/completed",
		"params": map[string]any{"threadId": "thread", "turn": map[string]any{"id": "turn", "status": "completed", "error": nil}},
	})
	d.Close()
	got := readDiagnosticManifest(t, dir)
	counts := got.Deltas["item/agentMessage/delta"]
	if counts.Events != count || counts.Bytes != count*len("汉 SECRET_CANARY") || counts.Samples != diagnosticsDeltaSamples || got.Events != diagnosticsDeltaSamples+1 || got.DroppedEvents != 0 || got.Truncated {
		t.Fatalf("delta aggregation did not preserve bounded evidence: %+v", got)
	}
	if got.LastTerminal == nil || got.LastTerminal.Status != "completed" || got.LastTerminal.TurnIDSHA256 != diagnosticIdentity("turn") || got.LastTerminal.Failure != (FailureInfo{}) {
		t.Fatalf("successful terminal missing or falsely failed: %+v", got.LastTerminal)
	}
	if strings.Contains(diagnosticFiles(t, dir), "SECRET_CANARY") {
		t.Fatal("sample or summary leaked secret")
	}
}

func TestDiagnosticsSummarySurvivesTraceQuotaAndBusinessFailure(t *testing.T) {
	dir := t.TempDir()
	d := NewDiagnostics(dir, "SECRET_CANARY")
	t.Cleanup(d.Close)
	d.maxEvents = 1
	d.Record("first", nil)
	for i := 0; i < 5000; i++ {
		d.Record("public_event", map[string]any{"method": "item/agentMessage/delta", "delta": "text"})
	}
	protocolDiagnostic(d, "notification", map[string]any{
		"method": "error", "params": map[string]any{"threadId": "thread", "turnId": "second", "willRetry": true,
			"error": map[string]any{"message": "SECRET_CANARY", "codexErrorInfo": map[string]any{"responseStreamDisconnected": map[string]any{"httpStatusCode": 502}}}},
	})
	protocolDiagnostic(d, "notification", map[string]any{
		"method": "turn/completed", "params": map[string]any{"threadId": "thread", "turn": map[string]any{"id": "second", "status": "completed", "error": nil}},
	})
	failure := failedObservation("search_answer_invalid", "private detail must not be copied")
	d.Record("sdk_cleanup", map[string]any{"process_reaped": true, "callback_work_pending": int64(0), "failed": false, "failure": nil})
	// Evidence.Completed can describe a completed turn even when a later
	// semantic check rejects its answer. Do not derive the verdict from it.
	d.Record("fixture_result", map[string]any{"stage": "search_turn_completed", "completed": true, "cleanup_pending": false, "failure": diagnosticFailure(failure)})
	detail := DescribeFailure(failure)
	d.Record("invocation_finished", map[string]any{"outcome": "not_observed", "reason": "scenario_not_completed", "failure": &detail, "exit_code": 1})
	d.Close()
	got := readDiagnosticManifest(t, dir)
	if got.Events != 1 || !got.Truncated || got.PendingWrites != 0 || !got.Closed || got.DroppedEvents == 0 {
		t.Fatalf("trace quota hidden: %+v", got)
	}
	if got.LastTerminal == nil || got.LastTerminal.Status != "completed" || got.LastTerminal.Failure != (FailureInfo{}) ||
		got.Result == nil || !got.Result.Completed || got.Result.Stage != "search_turn_completed" || got.Result.Failure != detail ||
		got.Cleanup == nil || !got.Cleanup.ProcessReaped || got.Cleanup.CallbackWorkPending != 0 || got.Cleanup.Failed ||
		got.Invocation == nil || got.Invocation.Outcome != "not_observed" || got.Invocation.ExitCode != 1 || got.Invocation.Failure != detail {
		t.Fatalf("terminal, business rejection and cleanup were conflated or lost: %+v", got)
	}
	if got.LastError == nil || got.LastError.WillRetry == nil || !*got.LastError.WillRetry || got.LastError.Failure.Code != "protocol_error" || got.LastError.Failure.ErrorKind != "responseStreamDisconnected" || got.LastError.Failure.HTTPStatus != 502 {
		t.Fatalf("retry failure category lost: %+v", got.LastError)
	}
	if strings.Contains(diagnosticFiles(t, dir), "SECRET_CANARY") || strings.Contains(diagnosticFiles(t, dir), "private detail") {
		t.Fatal("summary copied private error text")
	}
	if !reflect.DeepEqual(got, d.Status()) {
		t.Fatal("manifest and final Report snapshot disagree after joined writer")
	}
}

func TestDiagnosticsSummarySnapshotsAreBoundedImmutableAndClosed(t *testing.T) {
	d := NewDiagnostics(t.TempDir(), "SECRET_CANARY")
	t.Cleanup(d.Close)
	failure := &FailureInfo{Code: "SECRET_CANARY", ErrorKind: strings.Repeat("汉", 2000)}
	d.Record("fixture_result", map[string]any{"stage": strings.Repeat("汉", 2000), "failure": failure})
	d.Record("sdk_cleanup", map[string]any{"process_reaped": false, "callback_work_pending": int64(3), "failed": true, "failure": failure})
	d.Record("invocation_finished", map[string]any{"outcome": "not_observed", "failure": failure})
	d.Record("public_event", map[string]any{"method": "item/agentMessage/delta", "delta": "text"})
	protocolDiagnostic(d, "notification", map[string]any{"method": "turn/completed", "params": map[string]any{"turn": map[string]any{"id": "turn", "status": "failed", "error": nil}}})
	failure.Code = "CALLER_MUTATION"
	d.Close()
	before := d.Status()
	copy := d.Status()
	copy.Result.Stage = "RESULT_MUTATION"
	copy.Cleanup.CallbackWorkPending = 99
	copy.Invocation.Outcome = "OUTCOME_MUTATION"
	copy.LastTerminal.Status = "TERMINAL_MUTATION"
	copy.Deltas["item/agentMessage/delta"] = DiagnosticDelta{}
	if !reflect.DeepEqual(before, d.Status()) || before.Cleanup.CallbackWorkPending != 3 {
		t.Fatal("snapshot/caller mutation changed retained facts")
	}
	encoded, err := json.Marshal(before)
	if err != nil || len(encoded) > 16<<10 || strings.Contains(string(encoded), "CANARY") || strings.Contains(string(encoded), "MUTATION") {
		t.Fatal("summary exceeded its bound or leaked caller data")
	}
	d.Close()
	before = d.Status()
	d.Record("fixture_result", map[string]any{"stage": "after close", "completed": true})
	if d.Status().Result.Stage != before.Result.Stage || d.Status().DroppedEvents != before.DroppedEvents+1 {
		t.Fatal("closed capture admitted a new terminal fact")
	}
}

func TestDiagnosticsSummaryAvailableWhenTraceCannotBeWritten(t *testing.T) {
	path := filepath.Join(t.TempDir(), "blocked")
	if err := os.WriteFile(path, []byte("not a directory"), 0600); err != nil {
		t.Fatal(err)
	}
	d := NewDiagnostics(path)
	t.Cleanup(d.Close)
	d.Record("fixture_result", map[string]any{"stage": "finished", "completed": false, "failure": diagnosticFailure(errors.New("private generic failure"))})
	d.Record("invocation_finished", map[string]any{"outcome": "not_observed", "exit_code": 1})
	d.Close()
	got := d.Status()
	if got.Result == nil || got.Result.Failure.Code != "unclassified_failure" || got.Invocation == nil || got.Invocation.ExitCode != 1 || len(got.CaptureErrors) == 0 {
		t.Fatalf("capture failure hid final Report facts or invented SDK attribution: %+v", got)
	}
}

func TestDiagnosticsSummaryHashesIdentityAndPreservesUnknownRetry(t *testing.T) {
	dir := t.TempDir()
	d := NewDiagnostics(dir, "SECRET_CANARY")
	t.Cleanup(d.Close)
	protocolDiagnostic(d, "notification", map[string]any{
		"method": "turn/completed", "params": map[string]any{"threadId": "THREAD_ID_CANARY", "turn": map[string]any{"id": "TURN_ID_CANARY", "status": "PROVIDER_STATUS_CANARY", "error": nil}},
	})
	for _, retry := range []any{nil, "unknown"} {
		protocolDiagnostic(d, "notification", map[string]any{
			"method": "error", "params": map[string]any{"threadId": "THREAD_ID_CANARY", "turnId": "TURN_ID_CANARY", "willRetry": retry, "error": nil},
		})
		if got := d.Status().LastError; got == nil || got.WillRetry != nil {
			t.Fatal("missing or malformed retry indication was reported as false")
		}
	}
	protocolDiagnostic(d, "notification", map[string]any{
		"method": "error", "params": map[string]any{"threadId": "THREAD_ID_CANARY", "turnId": "TURN_ID_CANARY", "willRetry": true, "error": nil},
	})
	copy := d.Status()
	*copy.LastError.WillRetry = false
	if !*d.Status().LastError.WillRetry {
		t.Fatal("retry snapshot pointer alias changed retained facts")
	}
	for _, fragment := range []string{"SECRET_", "CANARY"} {
		protocolDiagnostic(d, "notification", map[string]any{"method": "item/agentMessage/delta", "params": map[string]any{"delta": fragment}})
	}
	d.Close()
	status := d.Status()
	encoded, err := json.Marshal(status)
	if err != nil || status.LastTerminal.Status != "unknown" || status.LastTerminal.ThreadIDSHA256 != diagnosticIdentity("THREAD_ID_CANARY") || status.LastTerminal.TurnIDSHA256 != diagnosticIdentity("TURN_ID_CANARY") {
		t.Fatalf("bounded summary classification/correlation: %+v", status)
	}
	for _, forbidden := range []string{"THREAD_ID_CANARY", "TURN_ID_CANARY", "PROVIDER_STATUS_CANARY", "SECRET_"} {
		if strings.Contains(string(encoded), forbidden) {
			t.Fatal("public status/report copied raw identity or provider content")
		}
	}
	trace, err := os.ReadFile(filepath.Join(dir, "events.jsonl"))
	if err != nil || !strings.Contains(string(trace), "THREAD_ID_CANARY") || strings.Contains(string(trace), `"delta":`) || strings.Contains(string(trace), "SECRET_") {
		t.Fatal("auxiliary correlation was lost or delta fragments were retained")
	}
}

func TestDiagnosticsHostedWebFailureSurvivesTraceLoss(t *testing.T) {
	wrapped := "stream disconnected before completion: " + failedWebDiagnostic
	raw := webDiagnosticError(t, "other", wrapped, nil)
	cause := failedTurn("failed", raw, "live: hosted turn did not complete")
	want := DescribeFailure(cause)
	for _, sink := range []string{"nil", "closed", "quota", "unwritable"} {
		t.Run(sink, func(t *testing.T) {
			var d *Diagnostics
			if sink != "nil" {
				path := t.TempDir()
				if sink == "unwritable" {
					path = filepath.Join(path, "occupied")
					if err := os.WriteFile(path, []byte("not a directory"), 0600); err != nil {
						t.Fatal(err)
					}
				}
				d = NewDiagnostics(path)
				t.Cleanup(d.Close)
				if sink == "closed" {
					d.Close()
				} else if sink == "quota" {
					d.maxEvents = 0
				}
			}
			retry := webDiagnosticError(t, map[string]any{"responseStreamDisconnected": map[string]any{"httpStatusCode": nil}}, "Reconnecting...", wrapped)
			protocolDiagnostic(d, "notification", map[string]any{"method": "error", "params": map[string]any{"error": retry, "willRetry": true}})
			protocolDiagnostic(d, "notification", map[string]any{"method": "turn/completed", "params": map[string]any{"turn": map[string]any{"id": "turn", "status": "failed", "error": raw}}})
			d.Record("fixture_result", map[string]any{"stage": "search_turn_submitted", "completed": false, "failure": diagnosticFailure(cause)})
			d.Record("invocation_finished", map[string]any{"outcome": "not_observed", "failure": want, "exit_code": 1})
			d.Close()
			if cause == nil || cause.Error() != "live: hosted turn did not complete" || DescribeFailure(cause) != want {
				t.Fatal("optional diagnostics changed the failed-turn outcome")
			}
			if sink == "quota" || sink == "unwritable" {
				got := d.Status()
				if got.LastError == nil || got.LastTerminal == nil || got.Result == nil || got.Invocation == nil ||
					got.LastError.Failure.HostedWeb != want.HostedWeb || got.LastTerminal.Failure != want || got.Result.Failure != want || got.Invocation.Failure != want || len(got.CaptureErrors) == 0 {
					t.Fatalf("trace loss discarded finite hosted-web classification: %+v", got)
				}
				encoded, err := json.Marshal(got)
				if err != nil || strings.Contains(string(encoded), "Grok stream:") || strings.Contains(string(encoded), "Reconnecting") {
					t.Fatal("compact summary copied error text instead of finite fields")
				}
			}
		})
	}
}

func TestDiagnosticsHostedWebFailureRemainsValueOnlyAndFinite(t *testing.T) {
	d := NewDiagnostics(t.TempDir())
	t.Cleanup(d.Close)
	detail := DescribeFailure(failedTurn("failed", webDiagnosticError(t, "other", failedWebDiagnostic, nil), "live: rejected"))
	d.Record("fixture_result", map[string]any{"failure": &detail})
	want := detail
	detail.HostedWeb.Action = "PRIVATE_CANARY"
	copy := d.Status()
	copy.Result.Failure.HostedWeb.Status = "PRIVATE_CANARY"
	if d.Status().Result.Failure != want {
		t.Fatal("hosted-web snapshot aliased caller or status state")
	}
	d.Record("fixture_result", map[string]any{"failure": &detail})
	d.Close()
	if d.Status().Result.Failure.HostedWeb != (HostedWebFailure{}) {
		t.Fatal("unrecognized caller-created fields bypassed the finite summary projection")
	}
	encoded, err := json.Marshal(d.Status())
	if err != nil || strings.Contains(string(encoded), "PRIVATE_CANARY") {
		t.Fatal("arbitrary diagnostic labels reached public summary")
	}
}
