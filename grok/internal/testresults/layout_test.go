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

// Keep CI's required owners tied to actual, tagged test implementations. Public
// production live APIs remain callable; forwarding to another package's test
// implementation does not provide a local business assertion.
func TestIntegrationPhysicalOwners(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("test source unavailable")
	}
	root := filepath.Join(filepath.Dir(file), "../..")
	owners := map[string]bool{}
	for _, owner := range IntegrationTests() {
		if owner.Package != "github.com/Harness-X-Harness/codex/grok/tests/integration" || strings.Contains(owner.Name, "/") {
			t.Fatalf("Integration owner is not a physical top-level test: %+v", owner)
		}
		owners[owner.Name] = false
	}
	functions := map[string]*ast.FuncDecl{}
	for _, area := range []string{"live", "tests/integration"} {
		dir := filepath.Join(root, area)
		entries, err := os.ReadDir(dir)
		if err != nil {
			t.Fatal(err)
		}
		for _, entry := range entries {
			if !strings.HasSuffix(entry.Name(), "_test.go") {
				continue
			}
			source, err := parser.ParseFile(token.NewFileSet(), filepath.Join(dir, entry.Name()), nil, parser.ParseComments)
			if err != nil {
				t.Fatal(err)
			}
			plain, tagged := build.Default, build.Default
			plain.BuildTags = nil
			tagged.BuildTags = []string{"integration"}
			offline, err := plain.MatchFile(dir, entry.Name())
			if err != nil {
				t.Fatal(err)
			}
			integration, err := tagged.MatchFile(dir, entry.Name())
			if err != nil {
				t.Fatal(err)
			}
			if area == "live" && integration != offline {
				t.Errorf("Integration-tagged implementation returned to live: %s", entry.Name())
			}
			for _, declaration := range source.Decls {
				fn, ok := declaration.(*ast.FuncDecl)
				if !ok || fn.Recv != nil {
					continue
				}
				name := fn.Name.Name
				if area == "live" {
					if _, owner := owners[name]; owner || strings.HasPrefix(name, "TestIntegration") {
						t.Errorf("Integration owner returned to live: %s", name)
					}
					continue
				}
				functions[name] = fn
				if name == "TestMain" || !strings.HasPrefix(name, "Test") {
					continue
				}
				seen, required := owners[name]
				if !required || seen || offline || !integration {
					t.Errorf("Unexpected, duplicate or incorrectly tagged Integration owner: %s", name)
				}
				owners[name] = true
			}
		}
	}
	for owner, found := range owners {
		if !found {
			t.Errorf("Required Integration implementation is absent: %s", owner)
			continue
		}
		seen := map[string]bool{}
		var asserted func(string) bool
		asserted = func(name string) bool {
			fn := functions[name]
			// Setup constructors returning a subject/server are not business
			// oracles for an otherwise assertion-free wrapper.
			if fn == nil || fn.Type.Results != nil || name == "integrationObservation" || name == "TestMain" || seen[name] {
				return false
			}
			seen[name] = true
			assertion := false
			ast.Inspect(fn, func(node ast.Node) bool {
				if selector, ok := node.(*ast.SelectorExpr); ok {
					if receiver, ok := selector.X.(*ast.Ident); ok && receiver.Name == "t" {
						switch selector.Sel.Name {
						case "Fatal", "Fatalf", "Error", "Errorf", "Fail", "FailNow":
							assertion = true
						}
					}
				}
				// Include local helpers passed to t.Run as well as direct calls.
				if ref, ok := node.(*ast.Ident); ok && functions[ref.Name] != nil && asserted(ref.Name) {
					assertion = true
				}
				return true
			})
			return assertion
		}
		if !asserted(owner) {
			t.Errorf("Required owner has no reachable assertion in tests/integration: %s", owner)
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
