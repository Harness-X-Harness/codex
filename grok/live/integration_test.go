//go:build integration

package live_test

import "testing"

// TestIntegration owns real Codex + controlled HTTP observations. Its helpers
// also support deterministic fixture-oracle tests without launching Codex.
func TestIntegration(t *testing.T) {
	t.Run("BasicFixture", testNativeBasicFixture)
	t.Run("StructuredEditFixture", runNativeStructuredEditFixture)
	t.Run("ReasoningHistoryFixture", testNativeReasoningHistoryFixture)
	t.Run("HostedSearchFixture", testNativeHostedSearchFixture)
	t.Run("ImagePinnedFixture", testNativeImagePinnedFixture)
	t.Run("ShippedCatalog", testNativeShippedCatalog)
}
