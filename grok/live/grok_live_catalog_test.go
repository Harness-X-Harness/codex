package live

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
)

const (
	shippedDefaultModel  = "grok-4.7"
	shippedPreviousModel = "grok-4.6"
)

func TestLiveHomeCopiesShippedCatalog(t *testing.T) {
	profile, err := os.ReadFile(shippedGrokProfilePath())
	if err != nil {
		t.Fatalf("read shipped profile: %v", err)
	}
	configured := topLevelTomlString(profile, "model")
	if configured != shippedDefaultModel {
		t.Fatalf("shipped profile model = %q, want %q", configured, shippedDefaultModel)
	}
	pinned := nonDefaultCatalogSlug(t, configured)
	if pinned != shippedPreviousModel {
		t.Fatalf("previous catalog model = %q, want %q", pinned, shippedPreviousModel)
	}

	home := t.TempDir()
	copyShippedModelCatalog(t, home, profile)
	rel := topLevelTomlString(profile, "model_catalog_json")
	installed, err := os.ReadFile(filepath.Join(home, rel))
	if err != nil {
		t.Fatalf("read installed catalog: %v", err)
	}
	source, err := os.ReadFile(filepath.Join(filepath.Dir(shippedGrokProfilePath()), rel))
	if err != nil {
		t.Fatalf("read source catalog: %v", err)
	}
	if !bytes.Equal(installed, source) {
		t.Fatal("installed catalog does not match grok/dist")
	}
}
