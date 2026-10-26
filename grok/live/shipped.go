package live

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/Harness-X-Harness/codex/grok/dist"
)

// ShippedCatalog starts the explicit subject with unchanged shipped assets and
// verifies the complete public catalog. It performs no inference.
func ShippedCatalog(ctx context.Context, subject Subject, key string, sinks ...*Diagnostics) (Evidence, error) {
	options, err := shippedOptions(subject, key, "")
	if err != nil {
		return Evidence{Stage: "preflight"}, err
	}
	if len(sinks) != 0 {
		options.Diagnostics = sinks[0]
	}
	options.catalogOnly = true
	return sdkShippedCatalog(ctx, options)
}

// ShippedStartup observes one turn from the unchanged shipped profile. Native
// tool composition is a C7 prerequisite; scripted proof alone is not Live success.
func ShippedStartup(ctx context.Context, subject Subject, key string, sinks ...*Diagnostics) (Evidence, error) {
	options, err := shippedOptions(subject, key, "")
	if err != nil {
		return Evidence{Stage: "preflight"}, err
	}
	if len(sinks) != 0 {
		options.Diagnostics = sinks[0]
	}
	return sdkShippedSession(ctx, options, func(server *sdkProductObserver, id string, evidence *Evidence) error {
		_, turn, err := observeShippedTurn(server, id, evidence, "", "Reply with a short confirmation that this turn completed.")
		if err == nil {
			server.finalize = func() error {
				for _, message := range server.queued() {
					if _, err := productTerminal(message, id, turn.ID); err != nil {
						return err
					}
				}
				return nil
			}
		}
		return err
	})
}

// ShippedPinned keeps the shipped previous-model route through the retained
// two-turn encrypted/tool-history scenario. It does not suppress shipped tools.
func ShippedPinned(ctx context.Context, subject Subject, key string, sinks ...*Diagnostics) (Evidence, error) {
	slugs, err := dist.ModelSlugs()
	if err != nil || len(slugs) != 2 {
		return Evidence{Stage: "preflight"}, errors.New("live: pinned shipped route unavailable")
	}
	options, err := shippedOptions(subject, key, slugs[1])
	if err != nil {
		return Evidence{Stage: "preflight"}, err
	}
	if len(sinks) != 0 {
		options.Diagnostics = sinks[0]
	}
	return ReasoningHistory(ctx, options)
}

func shippedOptions(subject Subject, key, selected string) (Options, error) {
	model, endpoint, err := dist.Defaults()
	if err != nil {
		return Options{}, errors.New("live: shipped policy unavailable")
	}
	if selected != "" {
		model = selected
	}
	return Options{Subject: subject, Model: model, BaseURL: endpoint, APIKey: key, shippedProfile: true}, nil
}

// This projects the bundled model policy for sameShippedCatalog's semantic
// comparison of identities, defaults, supported efforts and input capabilities.
func shippedModelList() (map[string]any, error) {
	var catalog struct {
		Models []map[string]any `json:"models"`
	}
	if json.Unmarshal(dist.Catalog(), &catalog) != nil {
		return nil, errors.New("live: shipped catalog unreadable")
	}
	rows := make([]any, 0, len(catalog.Models))
	for index, model := range catalog.Models {
		efforts := []any{}
		levels, ok := model["supported_reasoning_levels"].([]any)
		if !ok {
			return nil, errors.New("live: shipped efforts unavailable")
		}
		for _, value := range levels {
			level, ok := value.(map[string]any)
			if !ok {
				return nil, errors.New("live: invalid shipped effort")
			}
			efforts = append(efforts, map[string]any{"reasoningEffort": level["effort"], "description": level["description"]})
		}
		rows = append(rows, map[string]any{
			"id": model["slug"], "model": model["slug"], "displayName": model["display_name"], "description": model["description"],
			"upgrade": nil, "upgradeInfo": nil, "availabilityNux": nil, "modelSpecialty": nil,
			"hidden": false, "supportedReasoningEfforts": efforts, "defaultReasoningEffort": model["default_reasoning_level"],
			"inputModalities": model["input_modalities"], "supportsPersonality": false, "multiAgentVersion": model["multi_agent_version"],
			"additionalSpeedTiers": []any{}, "serviceTiers": []any{}, "defaultServiceTier": nil, "availableAccessPrograms": nil,
			"isDefault": index == 0,
		})
	}
	return map[string]any{"data": rows, "nextCursor": nil}, nil
}
