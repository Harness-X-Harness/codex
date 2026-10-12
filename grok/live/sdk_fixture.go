package live

import (
	"context"
	"errors"
	"os"
	"strings"
	"time"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

type sdkFixtureHandlers struct {
	notifications codexsdk.ServerNotificationHandler
	requests      codexsdk.ServerRequestHandler
	raw           *codexsdk.RawNotificationSubscription
	retention     codexsdk.NotificationRetentionBudget
	failures      <-chan error
}

// withSDKFixture owns one isolated SDK process; scenario observers retain
// responsibility for turn admission, correlation and business evidence.
func withSDKFixture(ctx context.Context, options Options, edit *editFixture, handlers sdkFixtureHandlers, run func(*codexsdk.Client, string, *Evidence) error) (Evidence, error) {
	return withFixture(ctx, options, edit, func(home, cwd string, evidence *Evidence) (runErr error) {
		experimental := true
		evidence.Stage = "sdk_starting"
		client, err := codexsdk.NewContext(ctx, codexsdk.ClientOptions{
			CWD: cwd, Command: []string{options.Subject.Binary, "app-server", "--strict-config", "--listen", "stdio://"},
			Env: sdkFixtureEnv(os.Environ(), home, options.APIKey),
			Initialize: protocolv2.InitializeParams{
				ClientInfo:   protocolv2.ClientInfo{Name: "grok-live", Version: "1"},
				Capabilities: protocolv2.Value(protocolv2.InitializeCapabilities{ExperimentalAPI: &experimental}),
			},
			NotificationRetention:     handlers.retention,
			ServerRequestHandler:      handlers.requests,
			ServerNotificationHandler: handlers.notifications,
			RawNotifications:          handlers.raw,
		})
		if err != nil {
			return sdkFailure(err)
		}
		defer func() {
			closeCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			closed, closeErr := client.CloseContext(closeCtx)
			if runErr == nil {
				select {
				case runErr = <-handlers.failures:
				default:
				}
				if runErr == nil && closeErr != nil {
					runErr = sdkFailure(closeErr)
				} else if runErr == nil && (!closed.ProcessReaped || closed.CallbackWorkPending != 0) {
					runErr = failedObservation("sdk_cleanup_incomplete", "live: SDK cleanup incomplete")
				}
			}
		}()
		evidence.Processes, evidence.Initializations, evidence.Stage = 1, 1, "initialized"
		identity := client.Provenance().RuntimeAppServer
		actualHome, homeErr := os.Stat(identity.CodexHome)
		fixtureHome, fixtureErr := os.Stat(home)
		if !identity.Observed || identity.UserAgent == "" || homeErr != nil || fixtureErr != nil || !os.SameFile(actualHome, fixtureHome) {
			return failedObservation("invalid_initialization", "live: initialization evidence unavailable")
		}
		return run(client, cwd, evidence)
	})
}

// sdkFixtureEnv replaces inherited fixture identity without mutating the parent.
func sdkFixtureEnv(parent []string, home, key string) []string {
	env := make([]string, 0, len(parent)+2)
	for _, entry := range parent {
		name, _, _ := strings.Cut(entry, "=")
		if !strings.EqualFold(name, "CODEX_HOME") && !strings.EqualFold(name, "GROK_API_KEY") {
			env = append(env, entry)
		}
	}
	return append(env, "CODEX_HOME="+home, "GROK_API_KEY="+key)
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
