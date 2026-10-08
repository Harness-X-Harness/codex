package facts

import (
	"context"
	"encoding/json"
	"errors"
	"strings"

	"github.com/Harness-X-Harness/codex/grok/internal/searchfixture"
)

// SearchObservation keeps wire admission and completed hosted replay distinct.
// It contains no prompt, tool input, URL, opaque identity or backend output.
type SearchObservation struct {
	Observation
	Scenario    string
	HostedCalls int
}

// SearchPolicy submits one fixed Web/X policy fixture. Completion establishes
// request admission, not a hosted call or enforcement of backend search results.
func (p *Probe) SearchPolicy(ctx context.Context, model, scenario string) (SearchObservation, error) {
	observed, _, err := p.search(ctx, model, scenario, "search_policy", nil)
	return observed, err
}

// SearchReplay submits one search and, only after a supported completed hosted
// call and final text, one follow-up replay. It never synthesizes local output.
func (p *Probe) SearchReplay(ctx context.Context, model, scenario string) (SearchObservation, error) {
	initial, output, err := p.search(ctx, model, scenario, "search_initial", nil)
	if err != nil {
		return initial, err
	}
	var calls []any
	for _, item := range output {
		if call, ok := searchfixture.ReplayCall(item, scenario); ok {
			if len(calls) == 64 {
				return initial, errors.New("hosted search evidence budget exceeded")
			}
			calls = append(calls, call)
		}
	}
	initial.HostedCalls = len(calls)
	if len(calls) == 0 {
		return initial, errors.New("completed hosted search absent")
	}
	replay, _, err := p.search(ctx, model, scenario, "search_replay", calls)
	replay.Requests += initial.Requests
	replay.HostedCalls = initial.HostedCalls
	replay.Replayed = true
	return replay, err
}

func (p *Probe) search(ctx context.Context, model, scenario, stage string, calls []any) (SearchObservation, []json.RawMessage, error) {
	observed := SearchObservation{Observation: Observation{Stage: stage}}
	tool, _, prompt, err := searchfixture.Plan(scenario)
	if err != nil || strings.TrimSpace(model) == "" {
		return observed, nil, errors.New("invalid hosted search fixture")
	}
	observed.Scenario = scenario
	if calls != nil {
		prompt = "Summarize the previous search result without searching again."
	}
	input := append(calls, map[string]any{"type": "message", "role": "user", "content": []any{map[string]any{"type": "input_text", "text": prompt}}})
	observation, raw, err := p.exchange(ctx, stage, map[string]any{"model": model, "stream": false, "input": input, "tools": []any{tool}})
	observed.Observation = observation
	if err != nil {
		return observed, nil, err
	}
	observed.Observation, _, err = observeResponse(observation, model, raw)
	if err != nil {
		return observed, nil, err
	}
	var result struct {
		Output []json.RawMessage `json:"output"`
	}
	if json.Unmarshal(raw, &result) != nil {
		return observed, nil, errors.New("invalid hosted search response")
	}
	// Every response, including the continuation, must preserve hosted isolation.
	for _, item := range result.Output {
		var kind struct {
			Type string `json:"type"`
		}
		_ = json.Unmarshal(item, &kind)
		switch kind.Type {
		case "function_call", "function_call_output", "custom_tool_call_output":
			return observed, nil, errors.New("local execution cannot establish hosted search")
		}
	}
	return observed, result.Output, nil
}
