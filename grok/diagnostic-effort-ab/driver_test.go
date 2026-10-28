package live

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/url"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/integrationfixture"
	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// This temporary diagnostic is opt-in and observes only the original first turn.
func TestEffortExperimentFirstTurn(t *testing.T) {
	arm := os.Getenv("GROK_EFFORT_ARM")
	if arm != "A" && arm != "B" {
		t.Fatal("experiment arm must be A or B")
	}
	endpoint, err := url.Parse(os.Getenv("GROK_EFFORT_BASE_URL"))
	if err != nil || endpoint.Scheme != "http" || endpoint.Hostname() != "127.0.0.1" || endpoint.Port() == "" || endpoint.User != nil || endpoint.RawQuery != "" || endpoint.Fragment != "" {
		t.Fatal("experiment requires loopback relay")
	}
	wrapper := os.Getenv("GROK_EFFORT_WRAPPER")
	resultPath := os.Getenv("GROK_EFFORT_RESULT")
	if !filepath.IsAbs(wrapper) || !filepath.IsAbs(resultPath) || os.Getenv("GROK_API_KEY") == "" {
		t.Fatal("experiment configuration incomplete")
	}
	contents, err := os.ReadFile(wrapper)
	if err != nil {
		t.Fatal("experiment wrapper unavailable")
	}
	sum := sha256.Sum256(contents)
	ctx, cancel := context.WithTimeout(context.Background(), 4*time.Minute)
	defer cancel()
	ctx, err = integrationfixture.WithShippedProfile(ctx, endpoint.String())
	if err != nil {
		t.Fatal("shipped endpoint configuration failed")
	}
	evidence, cause := effortExperimentFirstTurn(ctx, Options{
		shippedProfile: true,
		Model:          "grok-4.6", BaseURL: endpoint.String(), APIKey: os.Getenv("GROK_API_KEY"),
		Subject: Subject{Binary: wrapper, SHA256: hex.EncodeToString(sum[:]), Target: "x86_64-unknown-linux-musl", Environment: "effort-experiment", LocalUnverified: true},
	}, arm)
	result := map[string]any{
		"experiment": "frozen_initial_capture", "profile": "shipped", "header_policy": "native_three", "prompt_variant": "reason", "shipped_catalog": evidence.ShippedCatalog,
		"arm": arm, "model": "grok-4.6", "stage": evidence.Stage,
		"first_turn_completed": evidence.FirstCompleted,
		"first_token_matches":  evidence.FirstRecall,
		"reasoning_items":      evidence.ReasoningItems, "encrypted_items": evidence.EncryptedItems,
		"tool_calls": evidence.ToolCalls, "completed_tools": evidence.CompletedTools,
		"turns": evidence.Turns, "cleanup_pending": evidence.CleanupPending,
		"passed": cause == nil,
	}
	if cause != nil {
		result["failure"] = DescribeFailure(cause)
	}
	data, err := json.MarshalIndent(result, "", "  ")
	if err != nil || os.WriteFile(resultPath, append(data, '\n'), 0600) != nil {
		t.Fatal("experiment result write failed")
	}
	if cause != nil {
		t.Fatal("experiment first turn did not pass; see safe result")
	}
}

func effortExperimentFirstTurn(ctx context.Context, options Options, arm string) (Evidence, error) {
	schema, schemaErr := protocolv2.OutputSchemaFromJSON([]byte(historyOutputSchema))
	if schemaErr != nil {
		return Evidence{}, failedObservation("history_schema_invalid", "live: history output schema unavailable")
	}
	observer := newSDKHistoryObserver()
	observer.probe.diagnostics = options.Diagnostics
	handlers := sdkFixtureHandlers{
		requests: observer.request, notifications: observer.typedNotification, failures: observer.failures,
		retention: codexsdk.NotificationRetentionBudget{MaxCount: 65536, MaxBytes: 64 << 20},
		raw: &codexsdk.RawNotificationSubscription{
			Methods:                        []string{codexsdk.RawResponseItemCompletedExtension},
			EnableRawResponseItemExtension: true, Handler: observer.notification,
		},
	}
	evidence, err := withSDKFixture(ctx, options, nil, handlers, func(client *codexsdk.Client, cwd string, evidence *Evidence) error {
		if options.shippedProfile {
			if err := requireSDKShippedCatalog(ctx, client, evidence); err != nil {
				return err
			}
		}
		observer.mu.Lock()
		observer.observations = *evidence
		observer.observations.Threads = 1
		observer.mu.Unlock()
		defer func() {
			observer.mu.Lock()
			defer observer.mu.Unlock()
			*evidence = observer.observations
		}()
		prompt := "Call grok_history_probe, reason about its returned token, then return JSON with the exact token as the token field, or null if unavailable."
		turn := protocolv2.TurnStartParams{
			OutputSchema: &schema,
			Input: []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{
				Text: prompt,
			})},
		}
		options.Diagnostics.Record("turn_input", map[string]any{"scenario": "reasoning", "turn": 1, "model": options.Model, "input": turn.Input, "output_schema": json.RawMessage(historyOutputSchema)})
		experimental := true
		stream, err := client.ThreadRunner().StartStream(ctx, codexsdk.StartThreadRunRequest{
			Thread: protocolv2.ThreadStartParams{
				Model: protocolv2.Value(options.Model), ModelProvider: protocolv2.Value("grok"), CWD: protocolv2.Value(cwd),
				ExperimentalRawEvents: &experimental,
				DynamicTools: protocolv2.Value([]protocolv2.DynamicToolSpec{protocolv2.NewDynamicToolSpecFunction(protocolv2.DynamicToolSpecFunction{
					Name: historyTool, Description: integrationfixture.HistoryProbeDescription,
					InputSchema: protocolv2.JSONObject(map[string]protocolv2.JSONValue{
						"type": protocolv2.JSONString("object"), "properties": protocolv2.JSONObject(map[string]protocolv2.JSONValue{}),
						"additionalProperties": protocolv2.JSONBool(false),
					}),
				})}),
			},
			Turn: turn,
			AdmitTurn: func(thread protocolv2.ThreadStartResponse, _ protocolv2.TurnStartParams) error {
				if thread.Thread.ID == "" || thread.Model != options.Model || thread.ModelProvider != "grok" || thread.Thread.ModelProvider != "grok" || thread.CWD != cwd {
					return failedObservation("fixture_binding_failed", "live: fixture binding not established")
				}
				observer.mu.Lock()
				defer observer.mu.Unlock()
				observer.probe.threadID = thread.Thread.ID
				observer.observations.Bound, observer.observations.Stage = true, "thread_bound"
				observer.observations.Turns++
				observer.active = true
				return nil
			},
		})
		if err != nil {
			return observer.failure(err)
		}
		if err := observeSDKHistoryTurn(ctx, observer, stream, func(run codexsdk.StartedThreadRun) codexsdk.ThreadRunResult { return run.Run }); err != nil {
			return err
		}
		return nil
	})
	return settleSDKHistoryEvidence(evidence, err), err
}
