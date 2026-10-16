package live

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// sdkHostedSearch keeps the hosted Web/X oracle and replaces only its transport.
func sdkHostedSearch(ctx context.Context, options Options, scenario string, config map[string]any, prompt string) (SearchEvidence, error) {
	result := SearchEvidence{Scenario: scenario}
	transport := &sdkProductObserver{ctx: ctx, changed: make(chan struct{}, 1)}
	handlers := sdkFixtureHandlers{
		requests: transport.request, notifications: transport.typedNotification, afterClose: transport.afterClose,
		retention: codexsdk.NotificationRetentionBudget{MaxCount: 8192, MaxBytes: 256 << 20},
		// The business counter rejects the 4097th frame. The SDK default queue
		// of 64 closes the client before that frame is delivered.
		queue: 8192,
		raw: &codexsdk.RawNotificationSubscription{
			Methods:                        []string{codexsdk.RawResponseItemCompletedExtension},
			EnableRawResponseItemExtension: true, Handler: transport.rawNotification,
		},
	}
	base, err := withSDKFixture(ctx, options, nil, handlers, func(client *codexsdk.Client, cwd string, evidence *Evidence) error {
		transport.client = client
		encoded, err := json.Marshal(config)
		var decoded map[string]protocolv2.JSONValue
		if err != nil || json.Unmarshal(encoded, &decoded) != nil {
			return errors.New("live: invalid hosted search fixture")
		}
		experimental := true
		evidence.Threads = 1
		started, err := client.Threads().Start(ctx, protocolv2.ThreadStartParams{
			Model: protocolv2.Value(options.Model), ModelProvider: protocolv2.Value("grok"), CWD: protocolv2.Value(cwd),
			ExperimentalRawEvents: &experimental, ApprovalPolicy: protocolv2.Value(protocolv2.NewAskForApprovalNever()),
			Config: protocolv2.Value(decoded),
		})
		if err != nil {
			return shippedTransport(ctx, err)
		}
		if started.Model != options.Model || started.ModelProvider != "grok" || started.Thread.ModelProvider != "grok" ||
			started.Thread.ID == "" || started.CWD != cwd || started.Thread.CWD != cwd {
			return errors.New("live: fixture binding not established")
		}
		evidence.Bound, evidence.Stage = true, "thread_bound"
		probe := searchProbe{server: transport, threadID: started.Thread.ID, scenario: scenario, evidence: evidence, result: &result}
		if err := probe.observe(prompt); err != nil {
			return err
		}
		evidence.FirstCompleted, evidence.FirstReplyBytes = true, evidence.ReplyBytes
		if result.CanonicalCalls == 0 {
			return errors.New("live: completed hosted search absent")
		}
		evidence.Stage = "hosted_search_observed"
		probe.previousID = probe.turnID
		if err := probe.observe("Summarize the previous search result from this thread without searching again."); err != nil {
			return err
		}
		transport.finalize = func() error {
			for _, message := range transport.queued() {
				if err := probe.notification(message); err != nil {
					result.FollowupCompleted = false
					return err
				}
			}
			return nil
		}
		result.FollowupCompleted, evidence.Completed = true, true
		evidence.Stage = "hosted_continuation_completed"
		return nil
	})
	if err != nil {
		result.FollowupCompleted = false
	}
	result.Evidence = base
	return result, err
}
