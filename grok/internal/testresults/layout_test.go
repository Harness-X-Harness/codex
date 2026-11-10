package testresults

import (
	"go/ast"
	"go/build"
	"go/parser"
	"go/token"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/Harness-X-Harness/codex/grok/internal/scenarios"
)

// Required owners must exist and remain opt-in. Their source layout or use of
// shared helpers does not prove execution; JSON result lifecycles do.
func TestIntegrationPhysicalOwners(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("test source unavailable")
	}
	dir := filepath.Join(filepath.Dir(file), "../../tests/integration")
	owners := map[string]bool{}
	for _, owner := range IntegrationTests() {
		if owner.Package != "github.com/Harness-X-Harness/codex/grok/tests/integration" {
			t.Fatalf("unexpected Integration package identity: %+v", owner)
		}
		root, _, _ := strings.Cut(owner.Name, "/")
		owners[root] = false
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if !strings.HasSuffix(entry.Name(), "_test.go") {
			continue
		}
		source, err := parser.ParseFile(token.NewFileSet(), filepath.Join(dir, entry.Name()), nil, 0)
		if err != nil {
			t.Fatal(err)
		}
		plain, tagged := build.Default, build.Default
		plain.BuildTags, tagged.BuildTags = nil, []string{"integration"}
		offline, err := plain.MatchFile(dir, entry.Name())
		if err != nil {
			t.Fatal(err)
		}
		integration, err := tagged.MatchFile(dir, entry.Name())
		if err != nil {
			t.Fatal(err)
		}
		for _, declaration := range source.Decls {
			fn, ok := declaration.(*ast.FuncDecl)
			if !ok || fn.Recv != nil || fn.Name.Name == "TestMain" || !strings.HasPrefix(fn.Name.Name, "Test") {
				continue
			}
			if offline || !integration {
				t.Errorf("Integration execution is not opt-in: %s", fn.Name.Name)
			}
			if seen, required := owners[fn.Name.Name]; required {
				if seen {
					t.Errorf("Duplicate required Integration owner: %s", fn.Name.Name)
				}
				owners[fn.Name.Name] = true
			}
		}
	}
	for owner, found := range owners {
		if !found {
			t.Errorf("Required Integration implementation is absent: %s", owner)
		}
	}
}

// Catalog-generated CI owner names must still resolve to physical opt-in tests.
func TestCatalogPhysicalOwners(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("test source unavailable")
	}
	root := filepath.Join(filepath.Dir(file), "../..")
	for _, family := range []string{"live", "facts"} {
		t.Run(family, func(t *testing.T) {
			dir := filepath.Join(root, "tests", family)
			entries, err := os.ReadDir(dir)
			if err != nil {
				t.Fatal(err)
			}
			roots := map[string]bool{}
			for _, entry := range entries {
				if !strings.HasSuffix(entry.Name(), "_test.go") {
					continue
				}
				plain, tagged := build.Default, build.Default
				plain.BuildTags, tagged.BuildTags = nil, []string{family}
				offline, err := plain.MatchFile(dir, entry.Name())
				if err != nil {
					t.Fatal(err)
				}
				selected, err := tagged.MatchFile(dir, entry.Name())
				if err != nil || offline || !selected {
					t.Fatalf("Incorrectly tagged %s implementation: %s", family, entry.Name())
				}
				source, err := parser.ParseFile(token.NewFileSet(), filepath.Join(dir, entry.Name()), nil, 0)
				if err != nil {
					t.Fatal(err)
				}
				for _, declaration := range source.Decls {
					if fn, ok := declaration.(*ast.FuncDecl); ok && fn.Recv == nil {
						roots[fn.Name.Name] = true
					}
				}
			}
			selected, err := scenarios.Select(family, "diagnostic", "")
			if err != nil || len(selected) == 0 {
				t.Fatalf("Catalog selection is empty: %v", err)
			}
			for _, scenario := range selected {
				pkg, owner, ok := strings.Cut(scenario.TestOwner(), ":")
				testRoot, child, subtest := strings.Cut(owner, "/")
				if !ok || pkg != "github.com/Harness-X-Harness/codex/grok/tests/"+family || !roots[testRoot] || !subtest || child != scenario.Name {
					t.Errorf("Catalog owner has no matching physical %s test: %s", family, scenario.TestOwner())
				}
			}
		})
	}
}
