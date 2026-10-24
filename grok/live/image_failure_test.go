package live

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"strings"
	"testing"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

const imageFailureCanary = "PRIVATE_IMAGE_CANARY"

func newImageFailureProbe() *imageProbe {
	evidence := &ImageEvidence{Evidence: Evidence{Bound: true, Turns: 1, Stage: "image_turn_submitted"}}
	return &imageProbe{
		ctx: context.Background(), threadID: "thread", turnID: "turn",
		base: &evidence.Evidence, evidence: evidence, calls: map[string]*imageCall{},
		changed: make(chan struct{}, 1),
	}
}

func assertImageFailure(t *testing.T, err error, message string, want FailureInfo) {
	t.Helper()
	if err == nil || err.Error() != message {
		t.Fatalf("rejection message changed: %v; want %s", err, message)
	}
	got := DescribeFailure(fmt.Errorf("%s: %w", imageFailureCanary, err))
	if got != want {
		t.Fatalf("diagnostic = %+v, want %+v", got, want)
	}
	data, encodeErr := json.Marshal(got)
	if encodeErr != nil || strings.Contains(string(data)+err.Error(), imageFailureCanary) {
		t.Fatal("diagnostic retained private content")
	}
}

func TestImageFailureBoundTerminalDiagnostics(t *testing.T) {
	for _, tc := range []struct {
		name, status, raw string
		want              FailureInfo
	}{
		{"failed", "failed", `{"message":"PRIVATE_IMAGE_CANARY","codexErrorInfo":{"responseStreamDisconnected":{"httpStatusCode":429}}}`, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "responseStreamDisconnected", HTTPStatus: 429}},
		{"interrupted", "interrupted", "null", FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "interrupted"}},
		{"completed_error", "completed", `{"message":"PRIVATE_IMAGE_CANARY","codexErrorInfo":"badRequest"}`, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "completed", ErrorKind: "badRequest"}},
		{"unknown", imageFailureCanary, `{"codexErrorInfo":"PRIVATE_IMAGE_CANARY"}`, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "unknown", ErrorKind: "unknown"}},
		{"malformed", "failed", imageFailureCanary, FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "unknown"}},
		{"oversized", "failed", strings.Repeat(imageFailureCanary, 1024), FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "unknown"}},
	} {
		for _, route := range []string{"terminal", "start", "settled"} {
			t.Run(tc.name+"/"+route, func(t *testing.T) {
				probe := newImageFailureProbe()
				turn := imageTurn{ID: probe.turnID, Status: tc.status, Error: json.RawMessage(tc.raw)}
				message := "live: image turn did not complete successfully"
				var err error
				switch route {
				case "terminal":
					err = probe.terminal(turn, false)
				case "start":
					probe.turnID, probe.base.Stage = "", "thread_bound"
					err = probe.acceptStarted(turn)
				case "settled":
					err = probe.settle(probe.threadID, []imageTurn{turn})
					message = "live: settled image turn did not complete"
				}
				assertImageFailure(t, err, message, tc.want)
				if probe.completed || probe.base.FirstCompleted || probe.base.Completed || probe.evidence.TurnCompleted || probe.base.Stage != "image_turn_submitted" {
					t.Fatal("failed terminal gained completion credit or changed stage")
				}
			})
		}
	}
	probe := newImageFailureProbe()
	if err := probe.terminal(imageTurn{ID: probe.turnID, Status: "completed"}, false); err != nil || !probe.base.FirstCompleted || !probe.evidence.TurnCompleted || probe.base.Completed || probe.base.Stage != "image_turn_completed" {
		t.Fatal("successful terminal's partial proof changed")
	}
}

func TestImageFailureIdentityBeforeTerminalDiagnostics(t *testing.T) {
	turn := imageTurn{ID: imageFailureCanary, Status: "failed", Error: json.RawMessage(`{"codexErrorInfo":"unauthorized"}`)}
	probe := newImageFailureProbe()
	assertImageFailure(t, probe.terminal(turn, false), "live: image turn did not complete successfully", FailureInfo{Code: "image_turn_identity_mismatch"})
	turn.ID = ""
	assertImageFailure(t, probe.acceptStarted(turn), "live: image turn identity unavailable", FailureInfo{Code: "image_turn_identity_unavailable"})
	turn.ID = probe.turnID
	assertImageFailure(t, probe.settle(imageFailureCanary, []imageTurn{turn}), "live: settled image thread identity unavailable", FailureInfo{Code: "settled_image_thread_identity_unavailable"})
	turn.ID, turn.Items = imageFailureCanary, []imageItem{{Type: "imageGeneration"}}
	assertImageFailure(t, probe.settle(probe.threadID, []imageTurn{turn}), "live: unattributed image history", FailureInfo{Code: "unattributed_image_history"})
	turn.ID, turn.Items = probe.turnID, nil
	assertImageFailure(t, probe.settle(probe.threadID, []imageTurn{{ID: probe.turnID, Status: "completed"}, turn}), "live: settled image turn did not complete", FailureInfo{Code: "settled_image_turn_incomplete"})
}

func TestImageFailureTypedNotificationCorrelation(t *testing.T) {
	var terminalError protocolv2.TurnError
	if err := json.Unmarshal([]byte(`{"message":"PRIVATE_IMAGE_CANARY","additionalDetails":"https://private.invalid/?key=PRIVATE_IMAGE_CANARY","codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":401}}}`), &terminalError); err != nil {
		t.Fatal(err)
	}
	for _, tc := range []struct {
		name, thread, turn string
		want               bool
	}{
		{"current", "thread", "turn", true},
		{"prior", "thread", "prior", true},
		{"other_thread", imageFailureCanary, "turn", false},
		{"other_turn", "thread", imageFailureCanary, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			probe := newImageFailureProbe()
			probe.previousID = "prior"
			notification := protocolv2.NewServerNotificationTurnCompleted(protocolv2.ServerNotificationTurnCompleted{Params: protocolv2.TurnCompletedNotification{
				ThreadID: tc.thread, Turn: protocolv2.Turn{ID: tc.turn, Status: protocolv2.TurnStatusFailed, Error: protocolv2.Value(terminalError), Items: []protocolv2.ThreadItem{}},
			}})
			if _, err := json.Marshal(notification); err != nil {
				t.Fatal("terminal fixture must pass SDK envelope encoding")
			}
			if err := probe.typedNotification(context.Background(), notification); err != nil {
				t.Fatal("callback return behavior changed")
			}
			if !tc.want {
				if probe.cause != nil || probe.completed {
					t.Fatal("unrelated terminal changed the bound observation")
				}
				return
			}
			message := "live: image turn did not complete successfully"
			if tc.name == "prior" {
				message = "live: prior image turn did not complete successfully"
			}
			want := FailureInfo{Code: "turn_unsuccessful", TerminalStatus: "failed", ErrorKind: "httpConnectionFailed", HTTPStatus: 401}
			assertImageFailure(t, probe.wait(context.Background()), message, want)
			assertImageFailure(t, probe.afterClose("", probe.base, probe.cause), message, want)
			if probe.base.Completed || probe.base.FirstCompleted || probe.base.Stage != "image_turn_submitted" {
				t.Fatal("diagnostic callback created proof")
			}
		})
	}
}

func TestImageFailureStaticClasses(t *testing.T) {
	for _, tc := range []struct {
		name, code, message string
		run                 func(*imageProbe) error
	}{
		{"request", "unsupported_server_request", "live: unsupported server request", func(p *imageProbe) error {
			_, _ = p.request(context.Background(), protocolv2.ServerRequest{})
			return p.cause
		}},
		{"marshal", "invalid_image_evidence", "live: invalid image evidence", func(p *imageProbe) error {
			_ = p.observeNotification(context.Background(), frame{}, errors.New(imageFailureCanary))
			return p.cause
		}},
		{"frame", "invalid_image_evidence", "live: invalid image evidence", func(p *imageProbe) error {
			_ = p.observeFrame(context.Background(), "rawResponseItem/completed", []byte(imageFailureCanary), nil)
			return p.cause
		}},
		{"frame_budget", "image_evidence_budget_exceeded", "live: image evidence budget exceeded", func(p *imageProbe) error { p.frames = 4096; return p.notification(frame{}) }},
		{"byte_budget", "image_evidence_budget_exceeded", "live: image evidence budget exceeded", func(p *imageProbe) error { p.bytes = 64 << 20; return p.notification(frame{size: 1}) }},
		{"tool", "unexpected_image_tool_identity", "live: unexpected image tool identity", func(p *imageProbe) error { return p.raw(imageItem{Type: "function_call", Name: imageFailureCanary}) }},
		{"call_id", "image_call_identity_unavailable", "live: image call identity unavailable", func(p *imageProbe) error { _, err := p.record(""); return err }},
		{"image", "image_unsuccessful", "live: image did not complete successfully", func(p *imageProbe) error {
			return p.public(imageItem{Type: "imageGeneration", ID: imageFailureCanary, Status: "in_progress"}, false)
		}},
		{"payload", "invalid_image_base64", "live: invalid image base64", func(p *imageProbe) error { _, err := imagePayload(imageFailureCanary); return err }},
		{"output_url", "invalid_image_representation", "live: invalid canonical image representation", func(p *imageProbe) error {
			_, err := imageOutputDigest(json.RawMessage(`[{"type":"input_image","image_url":"https://private.invalid/?key=PRIVATE_IMAGE_CANARY"}]`))
			return err
		}},
		{"artifact_path", "image_artifact_outside_root", "live: image artifact outside save root", func(p *imageProbe) error {
			_, err := inspectImage(t.TempDir(), filepath.Join("..", imageFailureCanary), "")
			return err
		}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			probe := newImageFailureProbe()
			assertImageFailure(t, tc.run(probe), tc.message, FailureInfo{Code: tc.code})
			if probe.base.ToolCalls != 0 || probe.evidence.Images != 0 || probe.base.FirstCompleted || probe.base.Completed {
				t.Fatal("rejection gained image proof")
			}
		})
	}
}

func TestImageFailureDeferredValidationKeepsClassification(t *testing.T) {
	for _, tc := range []struct{ name, arguments, code, message string }{
		{"arguments", `{"prompt":"PRIVATE_IMAGE_CANARY","PRIVATE_IMAGE_CANARY":true}`, "invalid_image_arguments", "live: invalid image arguments"},
		{"generation_reference", `{"prompt":"PRIVATE_IMAGE_CANARY","referenced_image_paths":["PRIVATE_IMAGE_CANARY"]}`, "unexpected_generation_reference", "live: unexpected generation reference"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			probe := newImageFailureProbe()
			namespace := "image_gen"
			if err := probe.raw(imageItem{Type: "function_call", Name: "imagegen", Namespace: &namespace, CallID: "call", Arguments: tc.arguments}); err != nil {
				t.Fatal("validation diagnostic was no longer deferred")
			}
			assertImageFailure(t, probe.public(imageItem{Type: "imageGeneration", ID: "call"}, false), tc.message, FailureInfo{Code: tc.code})
			if probe.base.ToolCalls != 1 || probe.evidence.Images != 0 || probe.evidence.HistorySelected || probe.base.Completed {
				t.Fatal("deferred validation counters or proof changed")
			}
		})
	}
}

func TestImageFailureTransportAndUnknownErrors(t *testing.T) {
	for _, tc := range []struct {
		name, code, message string
		err                 error
	}{
		{"protocol", "protocol_request_failed", "live: protocol request failed", &codexsdk.ProtocolError{Message: imageFailureCanary, Method: imageFailureCanary}},
		{"closed", "protocol_ended_before_completion", "live: protocol ended before proof completion", codexsdk.ErrClientClosed},
		{"frame", "protocol_frame_budget_exceeded", "live: protocol frame budget exceeded", errors.New("JSON-RPC frame too large: 100 bytes")},
		{"unknown", "sdk_observation_failed", "live: SDK observation did not complete", errors.New(imageFailureCanary)},
	} {
		t.Run(tc.name, func(t *testing.T) {
			assertImageFailure(t, imageTransport(context.Background(), tc.err), tc.message, FailureInfo{Code: tc.code})
		})
	}
	for _, frameBudget := range []bool{false, true} {
		probe := newImageFailureProbe()
		done := make(chan struct{})
		close(done)
		probe.clientDone = done
		code, message := "protocol_ended_before_completion", "live: protocol ended before proof completion"
		if frameBudget {
			probe.closed = func() error { return errors.New("JSON-RPC frame too large: 100 bytes") }
			code, message = "protocol_frame_budget_exceeded", "live: protocol frame budget exceeded"
		}
		assertImageFailure(t, probe.wait(context.Background()), message, FailureInfo{Code: code})
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	assertImageFailure(t, newImageFailureProbe().wait(ctx), "live: image observation deadline exceeded", FailureInfo{Code: "image_observation_context_done"})
	assertImageFailure(t, imageTransport(ctx, errors.New(imageFailureCanary)), "live: image observation deadline exceeded", FailureInfo{Code: "image_observation_context_done"})
	for _, err := range []error{errors.New(imageFailureCanary), errors.New("live: invalid image evidence")} {
		probe := newImageFailureProbe()
		probe.cause = err
		if got := DescribeFailure(probe.wait(context.Background())); got != (FailureInfo{Code: "unclassified_failure"}) {
			t.Fatalf("untyped error text was classified: %+v", got)
		}
	}
}
