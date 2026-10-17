package live

import (
	"context"
	"github.com/Harness-X-Harness/codex/grok/internal/searchfixture"
)

// RunLegacySearchDiagnosticForTest is available only to this package's external
// fake-process tests. It cannot be called by product Live, CLI or Integration.
// Historical X decoder rejection is useful negative evidence, never a Story pass.
func RunLegacySearchDiagnosticForTest(ctx context.Context, options Options, scenario string) (SearchEvidence, error) {
	_, config, prompt, err := searchfixture.Plan(scenario)
	if err != nil {
		return SearchEvidence{}, err
	}
	return sdkHostedSearch(ctx, options, scenario, config, prompt)
}
