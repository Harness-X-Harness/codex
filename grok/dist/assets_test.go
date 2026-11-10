package dist

import (
	"strings"
	"testing"
)

func TestDefaultsUsesSelectedProviderTOMLSemantics(t *testing.T) {
	slugs, err := ModelSlugs()
	if err != nil {
		t.Fatal(err)
	}
	selected := slugs[0]
	original := profile
	t.Cleanup(func() { profile = original })
	for _, candidate := range []string{
		"model='selected-model' # selected\nmodel_provider = 'grok'\n[model_providers.grok]\nbase_url='https://example.test/v1'\n",
		"model = \"selected-model\"\nmodel_provider = \"grok\"\n[model_providers.other]\nbase_url = \"https://wrong.test/v1\"\n[model_providers.grok]\nbase_url = \"https://example.test/v1\"\n",
	} {
		profile = strings.ReplaceAll(candidate, "selected-model", selected)
		model, endpoint, err := Defaults()
		if err != nil || model != selected || endpoint != "https://example.test/v1" {
			t.Fatalf("selected profile semantics lost: %q %q %v", model, endpoint, err)
		}
	}
	for _, candidate := range []string{
		"model = \"selected-model\"\nmodel = \"selected-model\"\nmodel_provider = \"grok\"\n[model_providers.grok]\nbase_url = \"https://example.test/v1\"\n",
		"model = \"selected-model\"\nmodel_provider = \"grok\"\n[model_providers.other]\nbase_url = \"https://wrong.test/v1\"\n",
	} {
		profile = strings.ReplaceAll(candidate, "selected-model", selected)
		if _, _, err := Defaults(); err == nil {
			t.Fatal("ambiguous or missing selected provider accepted")
		}
	}
}
