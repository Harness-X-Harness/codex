// Package scenarios owns invocation, selection and budgets for Live and Facts.
// Facts observe the direct backend; they never substitute for product Live proof.
package scenarios

import (
	"context"
	"errors"
	"regexp"
	"strings"

	"github.com/Harness-X-Harness/codex/grok/facts"
	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

// Scenario is safe catalog metadata. SemanticTurns bounds submitted product turns
// for Live and direct HTTP requests for Facts; no runner retries failed work.
type Scenario struct {
	Name            string `json:"name"`
	Family          string `json:"family"`
	Model           string `json:"model"`
	Policy          string `json:"policy"`
	SemanticTurns   int    `json:"semantic_turns"`
	RequiresBinary  bool   `json:"requires_binary"`
	RequiresBackend bool   `json:"requires_backend"`
	Formal          bool   `json:"formal"`
	Smoke           bool   `json:"smoke"`
	liveRun         func(context.Context, live.Options) (LiveResult, error)
	factsRun        func(context.Context, *facts.Probe) (FactsResult, error)
}

// TestOwner names the physical tagged Go owner for this catalog scenario.
// The name is derived here so workflow consumers do not recreate family routing.
func (s Scenario) TestOwner() string {
	const root = "github.com/Harness-X-Harness/codex/grok/tests/"
	if s.Family == "live" {
		return root + "live:TestLive/" + s.Name
	}
	owner := "TestBackendFacts"
	if strings.HasPrefix(s.Policy, "web") || strings.HasPrefix(s.Policy, "x") {
		owner = "TestBackendSearchFacts"
	}
	return root + "facts:" + owner + "/" + s.Name
}

// LiveResult retains each existing scenario's full, safe evidence DTO.
type LiveResult struct {
	Evidence any
	Base     live.Evidence
}

// FactsResult retains the independent Facts CLI's observation shapes.
type FactsResult struct {
	Observation *facts.Observation
	Search      *facts.SearchObservation
	Routes      []facts.RouteObservation
}

// Completed reports the direct probe's completion without claiming product proof.
func (r FactsResult) Completed() bool {
	if r.Observation != nil {
		return r.Observation.Completed
	}
	if r.Search != nil {
		return r.Search.Completed
	}
	if len(r.Routes) == 0 {
		return false
	}
	for _, route := range r.Routes {
		if !route.Observation.Completed {
			return false
		}
	}
	return true
}

// LastObservation preserves the legacy Facts failure attribution boundary.
func (r FactsResult) LastObservation() facts.Observation {
	if r.Observation != nil {
		return *r.Observation
	}
	if r.Search != nil {
		return r.Search.Observation
	}
	if len(r.Routes) != 0 {
		return r.Routes[len(r.Routes)-1].Observation
	}
	return facts.Observation{}
}

// RunLive invokes exactly one owned product scenario with its catalog model.
func (s Scenario) RunLive(ctx context.Context, options live.Options) (LiveResult, error) {
	if s.Family != "live" || s.liveRun == nil {
		return LiveResult{}, errors.New("scenario is not Live")
	}
	options.Model = s.Model
	return s.liveRun(ctx, options)
}

// RunFacts invokes exactly one owned direct-backend scenario.
func (s Scenario) RunFacts(ctx context.Context, probe *facts.Probe) (FactsResult, error) {
	if s.Family != "facts" || s.factsRun == nil || probe == nil {
		return FactsResult{}, errors.New("scenario is not Facts")
	}
	return s.factsRun(ctx, probe)
}

// Select rejects stale or empty selections. Full contains only formal cases;
// diagnostic permits all known cases, including the exploratory no-effort case.
func Select(family, suite, name string) ([]Scenario, error) {
	if family != "live" && family != "facts" {
		return nil, errors.New("unknown scenario family")
	}
	if suite != "full" && suite != "smoke" && suite != "diagnostic" {
		return nil, errors.New("unknown scenario suite")
	}
	if err := validate(catalog); err != nil {
		return nil, err
	}
	var selected []Scenario
	for _, scenario := range catalog {
		if scenario.Family != family || (name != "" && scenario.Name != name) {
			continue
		}
		if suite == "full" && !scenario.Formal || suite == "smoke" && !scenario.Smoke {
			continue
		}
		selected = append(selected, scenario)
	}
	if len(selected) == 0 {
		return nil, errors.New("unknown or empty scenario selection")
	}
	return selected, nil
}

// Lookup resolves one stable name, including explicitly selected diagnostics.
func Lookup(family, name string) (Scenario, error) {
	if name == "" {
		return Scenario{}, errors.New("scenario required")
	}
	selected, err := Select(family, "diagnostic", name)
	if err != nil {
		return Scenario{}, err
	}
	return selected[0], nil
}

var stableName = regexp.MustCompile(`^[a-z][a-z0-9-]*$`)

func validate(entries []Scenario) error {
	if len(entries) == 0 {
		return errors.New("empty scenario catalog")
	}
	seen := make(map[string]bool, len(entries))
	for _, s := range entries {
		key := s.Family + "/" + s.Name
		if !stableName.MatchString(s.Name) || seen[key] || s.Policy == "" || s.Model == "" || s.SemanticTurns < 0 || s.Smoke && !s.Formal {
			return errors.New("invalid or duplicate scenario metadata")
		}
		seen[key] = true
		if s.Family == "live" {
			if s.liveRun == nil || s.factsRun != nil || !s.RequiresBinary {
				return errors.New("invalid Live dispatcher")
			}
		} else if s.Family == "facts" {
			if s.factsRun == nil || s.liveRun != nil || s.RequiresBinary || !s.RequiresBackend {
				return errors.New("invalid Facts dispatcher")
			}
		} else {
			return errors.New("unknown catalog family")
		}
	}
	return nil
}

func liveCase(name, model, policy string, turns int, smoke bool, run func(context.Context, live.Options) (LiveResult, error)) Scenario {
	return Scenario{Name: name, Family: "live", Model: model, Policy: policy, SemanticTurns: turns, RequiresBinary: true, RequiresBackend: turns > 0, Formal: true, Smoke: smoke, liveRun: run}
}

func plain(run func(context.Context, live.Options) (live.Evidence, error)) func(context.Context, live.Options) (LiveResult, error) {
	return func(ctx context.Context, options live.Options) (LiveResult, error) {
		evidence, err := run(ctx, options)
		return LiveResult{Evidence: evidence, Base: evidence}, err
	}
}

func edit(run func(context.Context, live.Options) (live.StructuredEditEvidence, error)) func(context.Context, live.Options) (LiveResult, error) {
	return func(ctx context.Context, options live.Options) (LiveResult, error) {
		evidence, err := run(ctx, options)
		return LiveResult{Evidence: evidence, Base: evidence.Evidence}, err
	}
}

func image(ctx context.Context, options live.Options) (LiveResult, error) {
	evidence, err := live.ImageGenerationEdit(ctx, options)
	return LiveResult{Evidence: evidence, Base: evidence.Evidence}, err
}

func shipped(run func(context.Context, live.Subject, string) (live.Evidence, error)) func(context.Context, live.Options) (LiveResult, error) {
	return plain(func(ctx context.Context, options live.Options) (live.Evidence, error) {
		return run(ctx, options.Subject, options.APIKey)
	})
}

func search(policy string) func(context.Context, live.Options) (LiveResult, error) {
	return func(ctx context.Context, options live.Options) (LiveResult, error) {
		evidence, err := live.HostedSearch(ctx, options, policy)
		return LiveResult{Evidence: evidence, Base: evidence.Evidence}, err
	}
}

func factsCase(name, model, policy string, requests int, smoke bool, run func(context.Context, *facts.Probe) (FactsResult, error)) Scenario {
	return Scenario{Name: name, Family: "facts", Model: model, Policy: policy, SemanticTurns: requests, RequiresBackend: true, Formal: true, Smoke: smoke, factsRun: run}
}

func text(model string, replay bool) func(context.Context, *facts.Probe) (FactsResult, error) {
	return func(ctx context.Context, probe *facts.Probe) (FactsResult, error) {
		run := probe.Text
		if replay {
			run = probe.EncryptedReplay
		}
		observation, err := run(ctx, model)
		return FactsResult{Observation: &observation}, err
	}
}

func factsSearch(policy string, replay bool) func(context.Context, *facts.Probe) (FactsResult, error) {
	return func(ctx context.Context, probe *facts.Probe) (FactsResult, error) {
		run := probe.SearchPolicy
		if replay {
			run = probe.SearchReplay
		}
		observation, err := run(ctx, providerfixture.PrimaryModel, policy)
		return FactsResult{Search: &observation}, err
	}
}

var catalog = []Scenario{
	liveCase("basic-primary", providerfixture.PrimaryModel, "text", 1, true, plain(live.Basic)),
	liveCase("basic-pinned", providerfixture.PinnedModel, "text", 1, false, plain(live.Basic)),
	liveCase("reasoning-primary", providerfixture.PrimaryModel, "reasoning-default", 2, true, plain(live.ReasoningHistory)),
	liveCase("reasoning-pinned", providerfixture.PinnedModel, "reasoning-default", 2, false, plain(live.ReasoningHistory)),
	liveCase("structured-edit", providerfixture.PrimaryModel, "edit", 2, false, edit(live.StructuredEdit)),
	liveCase("structured-edit-declined", providerfixture.PrimaryModel, "edit-declined", 1, false, edit(live.StructuredEditApprovalDeclined)),
	liveCase("structured-edit-pinned", providerfixture.PinnedModel, "edit-pinned", 1, false, edit(live.StructuredEditPinnedPreviousModel)),
	liveCase("structured-edit-all", providerfixture.PrimaryModel, "edit-replace-all", 1, false, edit(live.StructuredEditReplaceAll)),
	liveCase("attached-image-pinned", providerfixture.PinnedModel, "attached-image-history", 2, false, plain(live.AttachedImageHistory)),
	liveCase("image-primary", providerfixture.PrimaryModel, "image-generate-edit", 2, false, image),
	liveCase("image-pinned", providerfixture.PinnedModel, "image-generate-edit", 2, false, image),
	liveCase("shipped-catalog", providerfixture.PrimaryModel, "shipped-catalog", 0, false, shipped(live.ShippedCatalog)),
	liveCase("shipped-startup", providerfixture.PrimaryModel, "shipped-startup", 1, false, shipped(live.ShippedStartup)),
	liveCase("shipped-pinned", providerfixture.PinnedModel, "shipped-pinned", 2, false, shipped(live.ShippedPinned)),
	liveCase("shipped-child", providerfixture.PrimaryModel, "shipped-child", 2, false, shipped(live.ShippedChildCollaboration)),
	liveCase("web", providerfixture.PrimaryModel, "web", 2, true, search("web")),
	liveCase("web-allowed", providerfixture.PrimaryModel, "web_allowed", 2, false, search("web_allowed")),
	liveCase("web-excluded", providerfixture.PrimaryModel, "web_excluded", 2, false, search("web_excluded")),
	factsCase("basic-primary", providerfixture.PrimaryModel, "text", 1, true, text(providerfixture.PrimaryModel, false)),
	factsCase("basic-pinned", providerfixture.PinnedModel, "text", 1, false, text(providerfixture.PinnedModel, false)),
	factsCase("encrypted-replay", providerfixture.PrimaryModel, "encrypted-replay", 2, true, text(providerfixture.PrimaryModel, true)),
	factsCase("shipped-routes", "shipped-catalog", "shipped-routes", 2, false, func(ctx context.Context, probe *facts.Probe) (FactsResult, error) {
		routes, err := probe.ShippedRoutes(ctx)
		return FactsResult{Routes: routes}, err
	}),
	factsCase("web", providerfixture.PrimaryModel, "web", 1, true, factsSearch("web", false)),
	factsCase("web-allowed", providerfixture.PrimaryModel, "web_allowed", 1, false, factsSearch("web_allowed", false)),
	factsCase("web-excluded", providerfixture.PrimaryModel, "web_excluded", 1, false, factsSearch("web_excluded", false)),
	factsCase("x", providerfixture.PrimaryModel, "x", 1, true, factsSearch("x", false)),
	factsCase("x-window", providerfixture.PrimaryModel, "x_window", 1, false, factsSearch("x_window", false)),
	factsCase("web-replay", providerfixture.PrimaryModel, "web", 2, false, factsSearch("web", true)),
	factsCase("x-replay", providerfixture.PrimaryModel, "x", 2, false, factsSearch("x", true)),
}
