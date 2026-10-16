package live

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/internal/testkit"
)

func subjectFixture(t *testing.T) Options {
	t.Helper()
	binary := filepath.Join(t.TempDir(), "codex")
	if err := os.WriteFile(binary, []byte("unchanged executable"), 0700); err != nil {
		t.Fatal(err)
	}
	digest, err := testkit.ExecutableSHA256(binary)
	if err != nil {
		t.Fatal(err)
	}
	return Options{Subject: Subject{Binary: binary, SHA256: digest, Target: "test-host", Environment: "local-integration", LocalUnverified: true}, Model: providerfixture.PrimaryModel, BaseURL: "http://fixture.invalid/v1", APIKey: "fixture"}
}

func TestSubjectProvenance(t *testing.T) {
	for _, tc := range []struct {
		name            string
		local           bool
		source, harness string
		valid           bool
	}{
		{"local", true, "", "", true},
		{"verified", false, strings.Repeat("a", 40), strings.Repeat("b", 40), true},
		{"unverified_missing_labels", false, "", "", false},
		{"local_claims_source", true, strings.Repeat("a", 40), "", false},
		{"local_claims_harness", true, "", strings.Repeat("b", 40), false},
		{"local_claims_both", true, strings.Repeat("a", 40), strings.Repeat("b", 40), false},
		{"bad_source", false, "bad", strings.Repeat("b", 40), false},
		{"bad_harness", false, strings.Repeat("a", 40), "bad", false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			options := subjectFixture(t)
			options.Subject.LocalUnverified, options.Subject.SourceSHA, options.Subject.HarnessSHA = tc.local, tc.source, tc.harness
			ctx, cancel := context.WithTimeout(context.Background(), time.Minute)
			defer cancel()
			called, homePath := false, ""
			got, err := withFixture(ctx, options, nil, func(home, cwd string, evidence *Evidence) error {
				called, homePath = true, home
				return nil
			})
			if (err == nil) != tc.valid || called != tc.valid {
				t.Fatalf("subject validation mismatch: %+v; %v", got, err)
			}
			if !tc.valid {
				return
			}
			if got.LocalUnverified != tc.local || got.SourceSHA != tc.source || got.HarnessSHA != tc.harness || got.SHA256 != options.Subject.SHA256 {
				t.Fatal("subject provenance changed")
			}
			encoded, err := json.Marshal(got)
			if err != nil || strings.Contains(string(encoded), `"LocalUnverified":true`) != tc.local {
				t.Fatal("local observation label missing or invented")
			}
			if _, err := os.Stat(homePath); !os.IsNotExist(err) {
				t.Fatal("isolated fixture was not removed")
			}
		})
	}
}

func TestSubjectRejectsChangedExecutable(t *testing.T) {
	for _, change := range []string{"before", "during", "cleanup", "removed", "failed_run"} {
		t.Run(change, func(t *testing.T) {
			options := subjectFixture(t)
			mutate := func() {
				if err := os.WriteFile(options.Subject.Binary, []byte("different executable"), 0700); err != nil {
					t.Fatal(err)
				}
			}
			if change == "before" {
				mutate()
			}
			ctx, cancel := context.WithTimeout(context.Background(), time.Minute)
			defer cancel()
			called := false
			got, err := withFixture(ctx, options, nil, func(home, cwd string, evidence *Evidence) error {
				called, evidence.Completed = true, true
				switch change {
				case "cleanup":
					defer mutate()
				case "removed":
					if err := os.Remove(options.Subject.Binary); err != nil {
						t.Fatal(err)
					}
				default:
					mutate()
				}
				if change == "failed_run" {
					return errors.New("scenario rejected")
				}
				return nil
			})
			if err == nil || got.Completed || called != (change != "before") || strings.Contains(err.Error(), options.Subject.Binary) {
				t.Fatalf("changed executable accepted or leaked: %+v; %v", got, err)
			}
			if change != "before" && got.Stage != "artifact_changed" {
				t.Fatal("post-cleanup executable change was not identified")
			}
		})
	}
}
