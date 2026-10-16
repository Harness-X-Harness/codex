//go:build integration

package live

import "testing"

func TestIntegrationInternal(t *testing.T) {
	t.Run("ShippedThreadSource", testNativeShippedThreadSource)
	t.Run("ShippedChildFixture", runNativeShippedChildFixture)
	t.Run("FirstRequestShape", testNativeFirstRequestShape)
}
