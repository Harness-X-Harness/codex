package facts

import (
	"context"
	"github.com/Harness-X-Harness/codex/grok/dist"
)

// RouteObservation binds safe diagnostics to the shipped request slug. A backend
// response alias stays diagnostic and cannot rewrite the profile or catalog.
type RouteObservation struct {
	Model       string
	Observation Observation
}

// ShippedRoutes submits one bounded Text observation for each shipped route,
// stopping at the first failure. It never resubmits a semantic request.
func (p *Probe) ShippedRoutes(ctx context.Context) ([]RouteObservation, error) {
	models, err := dist.ModelSlugs()
	if err != nil {
		return nil, err
	}
	observations := make([]RouteObservation, 0, len(models))
	for _, model := range models {
		observation, err := p.Text(ctx, model)
		observations = append(observations, RouteObservation{model, observation})
		if err != nil {
			return observations, err
		}
	}
	return observations, nil
}
