//go:build integration

package integration_test

import (
	"context"
	"os"
	"testing"

	"github.com/Harness-X-Harness/codex/grok/internal/integrationfixture"
	"github.com/Harness-X-Harness/codex/grok/live"
)

func nativeSubject(t *testing.T) live.Subject {
	t.Helper()
	selected, cleanup, err := integrationfixture.SelectSubject(os.Getenv)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if t.Failed() {
			t.Log("execution snapshot retained after failed observation; cleanup requires confirming child termination")
			return
		}
		cleanup()
	})
	return live.Subject(selected)
}

func withShippedProfile(t *testing.T, ctx context.Context, endpoint string) context.Context {
	t.Helper()
	scoped, err := integrationfixture.WithShippedProfile(ctx, endpoint)
	if err != nil {
		t.Fatal(err)
	}
	return scoped
}
