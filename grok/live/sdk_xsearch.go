package live

import (
	"context"
	"errors"

	"github.com/ronhuafeng/llm-go/codexsdk"
)

var (
	errSDKProtocolEnded      = errors.New("live: protocol ended before proof completion")
	errSDKXSearchUnsupported = failedObservation("sdk_xsearch_unsupported", `live: strict SDK rejected unsupported xSearch`)
)

// The pinned SDK exposes plain errors. Match only its complete, known decode
// paths; never classify provider text, a ProtocolError, or an unknown variant
// by substring. These strings are compared in memory, never copied to evidence.
func xSearchDecodeError(err error) bool {
	if err == nil {
		return false
	}
	var rpc *codexsdk.ProtocolError
	if errors.As(err, &rpc) {
		return false
	}
	switch err.Error() {
	case `codexsdk: decode item/started notification: decode ServerNotification.params: decode ItemStartedNotification.item: decode ThreadItem.type: unknown variant "xSearch"`,
		`codexsdk: decode item/completed notification: decode ServerNotification.params: decode ItemCompletedNotification.item: decode ThreadItem.type: unknown variant "xSearch"`:
		return true
	}
	return false
}

// Only the public shutdown result supplies rejection provenance. A run's generic
// transport end can mask that cause, but business failures cannot be replaced.
func sdkRejectedXSearch(ctx context.Context, closed codexsdk.ShutdownResult, closeErr, runErr error) bool {
	return ctx.Err() == nil && closed.ProcessReaped && closed.CallbackWorkPending == 0 &&
		(runErr == nil || runErr == errSDKProtocolEnded) && xSearchDecodeError(closed.Cause) && xSearchDecodeError(closeErr)
}

func acceptedXSearchRejection(e SearchEvidence, err error) bool {
	// Exact identity also rejects errors.Join(rejection, artifact_changed).
	return err == errSDKXSearchUnsupported && (e.Scenario == "x" || e.Scenario == "x_window") &&
		e.Bound && e.Processes == 1 && e.Initializations == 1 && e.Threads == 1 && e.Turns == 1 &&
		e.Stage == "search_turn_submitted" && !e.CleanupPending && !e.Completed && !e.FirstCompleted && !e.FollowupCompleted && e.SettledTurns == 0
}

// AcceptedXSearchRejection recognizes only the bounded expected-rejection
// contract of #408. It does not establish positive hosted-search completion.
func AcceptedXSearchRejection(e SearchEvidence, err error) bool {
	return e.ExpectedUnsupported && acceptedXSearchRejection(e, err)
}
