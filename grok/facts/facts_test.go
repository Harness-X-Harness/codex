package facts

import (
	"context"
	"fmt"
	"os"
	"strings"
	"testing"
)

const userPrompt = "Reply with the single word ok."

func requireFactsClient(t *testing.T) *factClient {
	t.Helper()
	if os.Getenv(factsEnv) != "1" {
		t.Skip("set GROK_FACTS=1 and GROK_API_KEY to run Grok backend Facts")
	}
	key := strings.TrimSpace(os.Getenv(apiKeyEnv))
	if key == "" {
		t.Fatal("GROK_API_KEY is required when GROK_FACTS=1")
	}
	client, err := newFactClient(key)
	if err != nil {
		t.Fatalf("FACT_UNREACHABLE fact=%s err=%s", t.Name(), redact(err.Error(), key))
	}
	return client
}

func userInput() []any {
	return []any{
		map[string]any{
			"type": "message",
			"role": "user",
			"content": []any{
				map[string]any{"type": "input_text", "text": userPrompt},
			},
		},
	}
}

func postFact(t *testing.T, client *factClient, payload map[string]any) (int, []byte) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), requestTimeout)
	defer cancel()
	status, body, err := client.post(ctx, payload)
	if err != nil {
		t.Fatalf("FACT_UNREACHABLE fact=%s err=%s", t.Name(), redact(err.Error(), client.apiKey))
	}
	return status, body
}

func assertRecorded(t *testing.T, recorded, observed class) {
	t.Helper()
	observed = class(redact(string(observed), strings.TrimSpace(os.Getenv(apiKeyEnv))))
	if recorded == "" {
		if os.Getenv(factsRecordEnv) == "1" {
			t.Logf("FACT %s observed=%s", t.Name(), observed)
			return
		}
		t.Fatalf("FACT_UNRECORDED fact=%s", t.Name())
	}
	if recorded != observed {
		t.Fatalf("FACT_FLIPPED fact=%s recorded=%s observed=%s", t.Name(), recorded, observed)
	}
}

func encryptedBlob(t *testing.T, client *factClient) string {
	t.Helper()
	status, body := postFact(t, client, map[string]any{
		"input":   userInput(),
		"include": []string{"reasoning.encrypted_content"},
	})
	if got := classify(status, body); got != classAccepted {
		t.Fatalf("FACT_UNREACHABLE fact=%s err=blob fetch %s", t.Name(), redact(string(got), client.apiKey))
	}
	blob := firstEncryptedContent(body)
	if blob == "" {
		t.Fatalf("FACT_UNREACHABLE fact=%s err=no encrypted_content", t.Name())
	}
	return blob
}

func reasoningReplay(blob string, content any) []any {
	return append([]any{
		map[string]any{
			"type":              "reasoning",
			"encrypted_content": blob,
			"content":           content,
			"summary":           []any{},
		},
	}, userInput()...)
}

func TestFactWebSearchExternalWebAccessRejected(t *testing.T) {
	const recorded class = "rejected:400/Argument not supported: external_web_access"
	client := requireFactsClient(t)
	status, body := postFact(t, client, map[string]any{
		"input": userInput(),
		"tools": []any{
			map[string]any{"type": "web_search", "external_web_access": true},
		},
	})
	assertRecorded(t, recorded, classify(status, body))
}

func TestFactReasoningNullContentWithBlobRejected(t *testing.T) {
	const recorded class = "rejected:400/Could not decode the compaction blob. Ensure it is unmodified from the compact response."
	client := requireFactsClient(t)
	blob := encryptedBlob(t, client)
	status, body := postFact(t, client, map[string]any{
		"input": reasoningReplay(blob, nil),
	})
	assertRecorded(t, recorded, classify(status, body))
}

func TestFactReasoningTypedContentWithBlob(t *testing.T) {
	const recorded class = "accepted"
	client := requireFactsClient(t)
	blob := encryptedBlob(t, client)
	status, body := postFact(t, client, map[string]any{
		"input": reasoningReplay(blob, []any{
			map[string]any{"type": "reasoning_text", "text": "x"},
		}),
	})
	assertRecorded(t, recorded, classify(status, body))
}

func TestFactFunctionStrict(t *testing.T) {
	const recorded class = "accepted"
	client := requireFactsClient(t)
	status, body := postFact(t, client, map[string]any{
		"input": userInput(),
		"tools": []any{
			map[string]any{
				"type":        "function",
				"name":        "fact_ok",
				"description": "Report ok.",
				"strict":      true,
				"parameters": map[string]any{
					"type": "object",
					"properties": map[string]any{
						"ok": map[string]any{"type": "boolean"},
					},
					"required":             []any{"ok"},
					"additionalProperties": false,
				},
			},
		},
	})
	assertRecorded(t, recorded, classify(status, body))
}

func TestFactInputStatusOnHostedItems(t *testing.T) {
	const recorded class = "custom_tool_call=accepted;web_search_call=accepted;image_generation_call=accepted"
	client := requireFactsClient(t)
	const callID = "call_fact_status"
	items := []struct {
		name  string
		input []any
	}{
		{"custom_tool_call", []any{
			map[string]any{"type": "custom_tool_call", "id": "ctc_fact_status", "status": "completed", "name": "apply_patch", "input": "", "call_id": callID},
			map[string]any{"type": "custom_tool_call_output", "call_id": callID, "output": "ok"},
		}},
		{"web_search_call", []any{
			map[string]any{"type": "web_search_call", "id": "ws_fact_status", "status": "completed", "action": map[string]any{"type": "search", "query": "xai"}},
		}},
		{"image_generation_call", []any{
			map[string]any{"type": "image_generation_call", "id": "ig_fact_status", "status": "completed", "result": nil},
		}},
	}
	parts := make([]string, 0, len(items))
	for _, item := range items {
		status, body := postFact(t, client, map[string]any{"input": append(item.input, userInput()...)})
		parts = append(parts, item.name+"="+string(classify(status, body)))
	}
	assertRecorded(t, recorded, class(strings.Join(parts, ";")))
}

func TestFactCustomToolCallReplayRequiresID(t *testing.T) {
	const recorded class = "rejected:422/Failed to deserialize the JSON body into the target type: input[0]: invalid \"custom_tool_call\" item: missing field `id`"
	client := requireFactsClient(t)
	const callID = "call_fact_no_id"
	status, body := postFact(t, client, map[string]any{
		"input": append([]any{
			map[string]any{"type": "custom_tool_call", "status": "completed", "name": "apply_patch", "input": "", "call_id": callID},
			map[string]any{"type": "custom_tool_call_output", "call_id": callID, "output": "ok"},
		}, userInput()...),
	})
	assertRecorded(t, recorded, classify(status, body))
}

func TestFactWebSearchCallReplayRequiresAction(t *testing.T) {
	const recorded class = "rejected:422/Failed to deserialize the JSON body into the target type: input[0]: invalid \"web_search_call\" item: missing field `action`"
	client := requireFactsClient(t)
	status, body := postFact(t, client, map[string]any{
		"input": append([]any{
			map[string]any{"type": "web_search_call", "id": "ws_fact_no_action", "status": "completed"},
		}, userInput()...),
	})
	assertRecorded(t, recorded, classify(status, body))
}

func TestFactParallelToolCallsStoreClientMetadata(t *testing.T) {
	const recorded class = "parallel_tool_calls=accepted;store=accepted;client_metadata=accepted"
	client := requireFactsClient(t)
	parts := []struct {
		name    string
		payload map[string]any
	}{
		{"parallel_tool_calls", map[string]any{"input": userInput(), "parallel_tool_calls": false}},
		{"store", map[string]any{"input": userInput(), "store": false}},
		{"client_metadata", map[string]any{"input": userInput(), "client_metadata": map[string]string{"k": "v"}}},
	}
	var b strings.Builder
	for i, part := range parts {
		if i > 0 {
			b.WriteByte(';')
		}
		status, body := postFact(t, client, part.payload)
		fmt.Fprintf(&b, "%s=%s", part.name, classify(status, body))
	}
	assertRecorded(t, recorded, class(b.String()))
}

func TestFactIncludeEncryptedReasoning(t *testing.T) {
	const recorded class = "accepted"
	client := requireFactsClient(t)
	status, body := postFact(t, client, map[string]any{
		"input":   userInput(),
		"include": []string{"reasoning.encrypted_content"},
	})
	observed := classify(status, body)
	if observed == classAccepted {
		observed = observeEncryptedReasoning(body)
	}
	assertRecorded(t, recorded, observed)
}

func TestFactWebSearchAllowedDomains(t *testing.T) {
	const recorded class = "accepted"
	client := requireFactsClient(t)
	status, body := postFact(t, client, map[string]any{
		"input": userInput(),
		"tools": []any{
			map[string]any{
				"type": "web_search",
				"filters": map[string]any{
					"allowed_domains": []string{"x.ai"},
				},
			},
		},
	})
	assertRecorded(t, recorded, classify(status, body))
}

func TestFactXSearchDateWindow(t *testing.T) {
	const recorded class = "accepted"
	client := requireFactsClient(t)
	status, body := postFact(t, client, map[string]any{
		"input": userInput(),
		"tools": []any{
			map[string]any{
				"type":      "x_search",
				"from_date": "2026-01-01",
				"to_date":   "2026-01-31",
			},
		},
	})
	assertRecorded(t, recorded, classify(status, body))
}

func TestFactTextVerbosityRejectedOrIgnored(t *testing.T) {
	const recorded class = "accepted"
	client := requireFactsClient(t)
	status, body := postFact(t, client, map[string]any{
		"input": userInput(),
		"text":  map[string]any{"verbosity": "low"},
	})
	assertRecorded(t, recorded, classify(status, body))
}
