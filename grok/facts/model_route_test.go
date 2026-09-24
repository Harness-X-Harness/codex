package facts

import (
	"fmt"
	"testing"
)

// Model-route Facts probe the production Grok Responses surface.
// They record acceptance only. They do not change the product catalog
// or assume grok-build resolves to one physical model.

const modelRouteTextPrompt = "Reply with the single word ok."

func TestFactModelRouteGrokBuild(t *testing.T) {
	// grok-build accepts the route, tool round trip, and history replay.
	// reasoning.effort is rejected; that is the recorded shape, not a client rewrite.
	// response.model is not pinned: this run returned grok-build.
	const recorded class = "requested=grok-build;response_model=present;text=accepted;reasoning=rejected:400/Model grok-build does not support parameter reasoningEffort.;tool=accepted;history=accepted"
	client := requireFactsClient(t)
	assertRecorded(t, recorded, probeModelRoute(t, client, "grok-build", false))
}

func TestFactModelRouteGrok47(t *testing.T) {
	// Requested id and response.model differ. grok-4.7-build is observation, not the client id.
	const recorded class = "requested=grok-4.7;response_model=grok-4.7-build;text=accepted;reasoning=accepted;tool=accepted;history=accepted;encrypted_replay=accepted"
	client := requireFactsClient(t)
	assertRecorded(t, recorded, probeModelRoute(t, client, "grok-4.7", true))
}

func TestFactModelRouteGrok46(t *testing.T) {
	const recorded class = "requested=grok-4.6;response_model=grok-4.6-build;text=accepted;reasoning=accepted;tool=accepted;history=accepted"
	client := requireFactsClient(t)
	assertRecorded(t, recorded, probeModelRoute(t, client, "grok-4.6", false))
}

func probeModelRoute(t *testing.T, client *factClient, model string, encryptedReplay bool) class {
	t.Helper()
	textStatus, textBody := postFact(t, client, map[string]any{
		"model": model,
		"input": modelRouteUser(modelRouteTextPrompt),
	})
	returned := responseModel(textBody)
	t.Logf("FACT_MODEL requested=%s response_model=%s", model, returned)

	reasoningStatus, reasoningBody := postFact(t, client, map[string]any{
		"model":     model,
		"input":     modelRouteUser(modelRouteTextPrompt),
		"reasoning": map[string]any{"effort": "high"},
		"include":   []string{"reasoning.encrypted_content"},
	})
	blob := firstEncryptedContent(reasoningBody)

	toolStatus, toolBody := postFact(t, client, map[string]any{
		"model":       model,
		"input":       modelRouteUser("Call fact_echo now with text set to ok. Do not answer in prose before that tool call."),
		"tool_choice": "required",
		"tools":       []any{factEchoTool()},
	})
	call := firstFunctionCall(toolBody)
	history := class("no_function_call")
	if call != nil {
		historyStatus, historyBody := postFact(t, client, map[string]any{
			"model": model,
			"input": functionCallReplay(call),
		})
		history = classify(historyStatus, historyBody)
	}

	obs := modelRouteObservation{
		requested:     model,
		responseModel: returned,
		pinResponse:   model != "grok-build",
		text:          classify(textStatus, textBody),
		reasoning:     classify(reasoningStatus, reasoningBody),
		tool:          classify(toolStatus, toolBody),
		history:       history,
	}
	if encryptedReplay {
		obs.encryptedReplay = encryptedReplayClass(t, client, model, blob)
	}
	return obs.asClass()
}

func encryptedReplayClass(t *testing.T, client *factClient, model, blob string) class {
	t.Helper()
	if blob == "" {
		return "no_blob"
	}
	status, body := postFact(t, client, map[string]any{
		"model": model,
		"input": []any{
			map[string]any{
				"type":              "reasoning",
				"encrypted_content": blob,
				"summary":           []any{},
			},
			modelRouteUser(modelRouteTextPrompt)[0],
		},
	})
	return classify(status, body)
}

type modelRouteObservation struct {
	requested       string
	responseModel   string
	pinResponse     bool
	text            class
	reasoning       class
	tool            class
	history         class
	encryptedReplay class
}

func (o modelRouteObservation) asClass() class {
	response := "absent"
	if o.responseModel != "" {
		if o.pinResponse {
			response = o.responseModel
		} else {
			response = "present"
		}
	}
	value := fmt.Sprintf(
		"requested=%s;response_model=%s;text=%s;reasoning=%s;tool=%s;history=%s",
		o.requested,
		response,
		o.text,
		o.reasoning,
		o.tool,
		o.history,
	)
	if o.encryptedReplay != "" {
		value += ";encrypted_replay=" + string(o.encryptedReplay)
	}
	return class(value)
}

func modelRouteUser(text string) []any {
	return []any{
		map[string]any{
			"type": "message",
			"role": "user",
			"content": []any{
				map[string]any{"type": "input_text", "text": text},
			},
		},
	}
}

func factEchoTool() map[string]any {
	return map[string]any{
		"type":        "function",
		"name":        "fact_echo",
		"description": "Echo a short token.",
		"parameters": map[string]any{
			"type": "object",
			"properties": map[string]any{
				"text": map[string]any{"type": "string"},
			},
			"required": []any{"text"},
		},
	}
}

func functionCallReplay(call map[string]any) []any {
	item := map[string]any{
		"type":      "function_call",
		"name":      jsonString(call["name"]),
		"arguments": jsonString(call["arguments"]),
		"call_id":   jsonString(call["call_id"]),
	}
	if id := jsonString(call["id"]); id != "" {
		item["id"] = id
	}
	return []any{
		modelRouteUser("Call fact_echo now with text set to ok. Do not answer in prose before that tool call.")[0],
		item,
		map[string]any{
			"type":    "function_call_output",
			"call_id": jsonString(call["call_id"]),
			"output":  "ok",
		},
		modelRouteUser(modelRouteTextPrompt)[0],
	}
}
