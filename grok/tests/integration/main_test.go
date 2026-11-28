//go:build integration

package integration_test

import (
	"flag"
	"fmt"
	"os"
	"strings"
	"sync/atomic"
	"testing"
)

var observations atomic.Uint64

// Discovery remains available without a binary. An explicit test invocation
// cannot succeed because all owners or all selected descendants were filtered.
func TestMain(m *testing.M) {
	flag.Parse()
	listing := flag.Lookup("test.list")
	if listing != nil && listing.Value.String() != "" {
		os.Exit(m.Run())
	}
	if flag.Lookup("test.skip").Value.String() != "" {
		fmt.Fprintln(os.Stderr, "integration: skipped selection forbidden; use -run")
		os.Exit(1)
	}
	if flag.Lookup("test.count").Value.String() != "1" || strings.Contains(flag.Lookup("test.cpu").Value.String(), ",") {
		fmt.Fprintln(os.Stderr, "integration: repeated or zero execution forbidden")
		os.Exit(1)
	}
	code := m.Run()
	if code == 0 && (listing == nil || listing.Value.String() == "") && observations.Load() == 0 {
		fmt.Fprintln(os.Stderr, "integration: no observations selected; choose a real Integration owner")
		code = 1
	}
	os.Exit(code)
}

func integrationObservation(t *testing.T) {
	t.Helper()
	observations.Add(1)
	t.Cleanup(func() {
		if t.Skipped() {
			t.Error("integration: selected observations cannot pass by skipping")
		}
	})
}
