package integrationfixture

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/Harness-X-Harness/codex/grok/dist"
	"github.com/pelletier/go-toml/v2"
)

func TestShippedEndpointPreservesSourceAssets(t *testing.T) {
	home := t.TempDir()
	if err := WriteShippedProfile(home, filepath.Join(home, "workspace")); err != nil {
		t.Fatal(err)
	}
	endpoint := "http://127.0.0.1:45123/v1"
	ctx, err := WithShippedProfile(context.Background(), endpoint)
	if err != nil {
		t.Fatal(err)
	}
	if got, ok := ShippedEndpoint(ctx); !ok || got != endpoint {
		t.Fatal("scoped endpoint missing")
	}
	if _, ok := ShippedEndpoint(context.Background()); ok {
		t.Fatal("endpoint escaped its context")
	}
	if err := SubstituteShippedEndpoint(home, endpoint); err != nil {
		t.Fatal(err)
	}
	config, err := os.ReadFile(filepath.Join(home, "config.toml"))
	if err != nil {
		t.Fatal(err)
	}
	var expected, actual map[string]any
	if toml.Unmarshal([]byte(dist.Profile()), &expected) != nil || toml.Unmarshal(config, &actual) != nil {
		t.Fatal("profile is not valid TOML")
	}
	providers := expected["model_providers"].(map[string]any)
	providers[expected["model_provider"].(string)].(map[string]any)["base_url"] = endpoint
	if !reflect.DeepEqual(actual, expected) {
		t.Fatal("profile policy changed")
	}
	catalog, err := os.ReadFile(filepath.Join(home, "models.json"))
	if err != nil || !bytes.Equal(catalog, dist.Catalog()) {
		t.Fatal("catalog source changed")
	}
	if err := SubstituteShippedEndpoint(home, endpoint); err == nil {
		t.Fatal("mutated profile must not be accepted as original source")
	}
}

func TestShippedEndpointRejectsUnsafeOrChangedInputs(t *testing.T) {
	for _, endpoint := range []string{"https://127.0.0.1:1/v1", "http://localhost:1/v1", "http://192.0.2.1:1/v1", "http://127.0.0.1/v1", "http://user@127.0.0.1:1/v1", "http://127.0.0.1:1/v1?secret=x", "http://127.0.0.1:1/other", "http://127.0.0.1:1/v1#fragment"} {
		if _, err := WithShippedProfile(context.Background(), endpoint); err == nil {
			t.Fatal("non-loopback or ambiguous fixture endpoint admitted")
		}
	}
	for _, asset := range []string{"config.toml", "models.json"} {
		t.Run(asset, func(t *testing.T) {
			home := t.TempDir()
			if err := WriteShippedProfile(home, filepath.Join(home, "workspace")); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(home, asset), []byte("changed"), 0600); err != nil {
				t.Fatal(err)
			}
			if err := SubstituteShippedEndpoint(home, "http://[::1]:45123/v1"); err == nil {
				t.Fatal("changed source asset accepted")
			}
		})
	}
}

func TestEnvironmentReplacesOnlyFixtureIdentity(t *testing.T) {
	parent := []string{"CODEX_HOME=old", "GROK_API_KEY=old", "keep=value", "codex_home=case-old", "grok_api_key=case-old"}
	got := Environment(parent, "owned-home", "fixture-key")
	want := []string{"keep=value", "CODEX_HOME=owned-home", "GROK_API_KEY=fixture-key"}
	if strings.Join(got, "\n") != strings.Join(want, "\n") || parent[0] != "CODEX_HOME=old" {
		t.Fatal("environment isolation changed parent or leaked inherited fixture identity")
	}
}
