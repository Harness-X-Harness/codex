package live

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"

	"github.com/Harness-X-Harness/codex/grok/dist"
)

// ShippedCatalog starts the explicit subject with unchanged shipped assets and
// verifies the complete public catalog. It performs no inference.
func ShippedCatalog(ctx context.Context, subject Subject, key string) (Evidence, error) {
	options, err := shippedOptions(subject, key, "")
	if err != nil {
		return Evidence{Stage: "preflight"}, err
	}
	options.catalogOnly = true
	return runFixture(ctx, options, nil, nil, nil)
}

// ShippedStartup observes one turn from the unchanged shipped profile. Native
// tool composition is a C7 prerequisite; scripted proof alone is not Live success.
func ShippedStartup(ctx context.Context, subject Subject, key string) (Evidence, error) {
	options, err := shippedOptions(subject, key, "")
	if err != nil {
		return Evidence{Stage: "preflight"}, err
	}
	return runFixture(ctx, options, nil, nil, func(server *appServer, id string, evidence *Evidence) error {
		_, _, err := observeShippedTurn(server, id, evidence, "", "Reply with a short confirmation that this turn completed.")
		return err
	})
}

// ShippedPinned keeps the shipped previous-model route through the retained
// two-turn encrypted/tool-history scenario. It does not suppress shipped tools.
func ShippedPinned(ctx context.Context, subject Subject, key string) (Evidence, error) {
	slugs, err := dist.ModelSlugs()
	if err != nil || len(slugs) != 2 {
		return Evidence{Stage: "preflight"}, errors.New("live: pinned shipped route unavailable")
	}
	options, err := shippedOptions(subject, key, slugs[1])
	if err != nil {
		return Evidence{Stage: "preflight"}, err
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

func writeShippedProfile(home, cwd string) error {
	if os.Mkdir(cwd, 0700) != nil || os.WriteFile(filepath.Join(home, "config.toml"), []byte(dist.Profile()), 0600) != nil || os.WriteFile(filepath.Join(home, "models.json"), dist.Catalog(), 0600) != nil {
		return errors.New("live: shipped profile copy failed")
	}
	return nil
}

func requireShippedCatalog(server *appServer, evidence *Evidence) error {
	var actual map[string]any
	if err := server.call("model/list", map[string]any{"limit": 100}, &actual); err != nil {
		return err
	}
	expected, err := shippedModelList()
	if err != nil || !reflect.DeepEqual(actual, expected) {
		return errors.New("live: complete shipped catalog mismatch")
	}
	evidence.CatalogModels, evidence.ShippedCatalog = len(expected["data"].([]any)), true
	evidence.Stage = "shipped_catalog_verified"
	return nil
}

// This is the stock Model DTO projection, including explicit null and empty
// values. Comparing the whole response rejects extra/reordered/duplicate rows,
// metadata loss, pagination and additional defaults.
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
