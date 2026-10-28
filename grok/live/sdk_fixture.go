package live

import (
	"context"
	"errors"
	"os"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/integrationfixture"
	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

var errSDKProtocolEnded = errors.New("live: protocol ended before proof completion")

type sdkFixtureHandlers struct {
	notifications codexsdk.ServerNotificationHandler
	requests      codexsdk.ServerRequestHandler
	raw           *codexsdk.RawNotificationSubscription
	retention     codexsdk.NotificationRetentionBudget
	// queue is the SDK notification admission buffer. Zero keeps the SDK
	// default. Image bursts must fit so the business frame budget, not the
	// transport queue, decides overflow.
	queue    int
	failures <-chan error
	// afterClose runs after CloseContext joins callbacks and reaps the child,
	// while the isolated workspace still exists. Only invoked when cleanup is complete.
	afterClose func(cwd string, evidence *Evidence, cause error) error
}

// withSDKFixture owns one isolated SDK process; scenario observers retain
// responsibility for turn admission, correlation and business evidence.
func withSDKFixture(ctx context.Context, options Options, edit *editFixture, handlers sdkFixtureHandlers, run func(*codexsdk.Client, string, *Evidence) error) (Evidence, error) {
	return withFixture(ctx, options, edit, func(home, cwd string, evidence *Evidence) (runErr error) {
		options.Diagnostics.Record("sdk_starting", map[string]any{"model": options.Model, "stderr_capture": "unavailable_pinned_sdk_discards_stderr"})
		notifications := handlers.notifications
		handlers.notifications = func(ctx context.Context, notification protocolv2.ServerNotification) error {
			protocolDiagnostic(options.Diagnostics, "notification", notification)
			if notifications != nil {
				return notifications(ctx, notification)
			}
			return nil
		}
		requests := handlers.requests
		if requests != nil {
			handlers.requests = func(ctx context.Context, request protocolv2.ServerRequest) (codexsdk.ServerRequestResponse, error) {
				protocolDiagnostic(options.Diagnostics, "server_request", request)
				response, err := requests(ctx, request)
				options.Diagnostics.Record("server_request_handled", map[string]any{"kind": request.Kind(), "failed": err != nil, "failure": diagnosticFailure(err)})
				return response, err
			}
		}
		experimental := true
		evidence.Stage = "sdk_starting"
		// NewContext may return before its asynchronous failure teardown, and
		// provides no client on that path. Retain inputs until a public join is known.
		evidence.CleanupPending = true
		client, err := codexsdk.NewContext(ctx, codexsdk.ClientOptions{
			CWD: cwd, Command: []string{options.Subject.Binary, "app-server", "--strict-config", "--listen", "stdio://"},
			Env: integrationfixture.Environment(os.Environ(), home, options.APIKey),
			Initialize: protocolv2.InitializeParams{
				ClientInfo:   protocolv2.ClientInfo{Name: "grok-live", Version: "1"},
				Capabilities: protocolv2.Value(protocolv2.InitializeCapabilities{ExperimentalAPI: &experimental}),
			},
			NotificationRetention:     handlers.retention,
			NotificationQueueCapacity: handlers.queue,
			ServerRequestHandler:      handlers.requests,
			ServerNotificationHandler: handlers.notifications,
			RawNotifications:          handlers.raw,
		})
		if err != nil {
			options.Diagnostics.Record("sdk_start_failed", map[string]any{"failure": DescribeFailure(sdkFailure(err))})
			return sdkFailure(err)
		}
		defer func() {
			closeCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			closed, closeErr := client.CloseContext(closeCtx)
			evidence.CleanupPending = !closed.ProcessReaped || closed.CallbackWorkPending != 0
			options.Diagnostics.Record("sdk_cleanup", map[string]any{"process_reaped": closed.ProcessReaped, "callback_work_pending": closed.CallbackWorkPending, "failed": closeErr != nil, "failure": diagnosticFailure(closeErr)})
			var callbackErr error
			select {
			case callbackErr = <-handlers.failures:
			default:
			}
			if callbackErr != nil {
				options.Diagnostics.Record("callback_failure", map[string]any{"failure": DescribeFailure(callbackErr)})
			}
			if runErr == nil {
				runErr = callbackErr
				if runErr == nil && closeErr != nil {
					runErr = sdkFailure(closeErr)
				} else if runErr == nil && evidence.CleanupPending {
					runErr = failedObservation("sdk_cleanup_incomplete", "live: SDK cleanup incomplete")
				}
			}
			if handlers.afterClose != nil && !evidence.CleanupPending {
				if finalized := handlers.afterClose(cwd, evidence, runErr); finalized != nil {
					runErr = preferObservation(runErr, finalized)
				}
			}
		}()
		evidence.Processes, evidence.Initializations, evidence.Stage = 1, 1, "initialized"
		options.Diagnostics.Record("sdk_initialized", map[string]any{"model": options.Model})
		identity := client.Provenance().RuntimeAppServer
		actualHome, homeErr := os.Stat(identity.CodexHome)
		fixtureHome, fixtureErr := os.Stat(home)
		if !identity.Observed || identity.UserAgent == "" || homeErr != nil || fixtureErr != nil || !os.SameFile(actualHome, fixtureHome) {
			return failedObservation("invalid_initialization", "live: initialization evidence unavailable")
		}
		return run(client, cwd, evidence)
	})
}

// sdkFailure never formats SDK errors, which can contain private protocol data.
func sdkFailure(err error) error {
	var observation *observationFailure
	if errors.As(err, &observation) {
		return observation
	}
	code := "sdk_observation_failed"
	switch {
	case errors.Is(err, codexsdk.ErrNotificationRetention):
		code = "notification_retention_exceeded"
	case errors.Is(err, codexsdk.ErrNotificationBackpressure):
		code = "notification_backpressure"
	case errors.Is(err, context.Canceled):
		code = "cancelled"
	case errors.Is(err, context.DeadlineExceeded):
		code = "deadline_exceeded"
	}
	return failedObservation(code, "live: SDK observation did not complete")
}

// preferObservation keeps an earlier business failure ahead of a later generic
// shutdown wrapper, and lets a sticky observation replace a transport error.
func preferObservation(current, finalized error) error {
	if current == nil || current == errSDKProtocolEnded {
		return finalized
	}
	var currentObservation, finalObservation *observationFailure
	if errors.As(finalized, &finalObservation) && !errors.As(current, &currentObservation) {
		return finalized
	}
	return current
}
