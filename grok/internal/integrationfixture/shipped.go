package integrationfixture

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"net"
	"net/url"
	"os"
	"path/filepath"
	"strings"

	"github.com/Harness-X-Harness/codex/grok/dist"
)

// Test-only, after the ordinary shipped writer. Verify and reread both assets:
// controlled transport must not silently weaken any shipped capability.
func SubstituteShippedEndpoint(home, endpoint string) error {
	if err := validateEndpoint(endpoint); err != nil {
		return err
	}
	_, original, err := dist.Defaults()
	if err != nil {
		return err
	}
	path := filepath.Join(home, "config.toml")
	profile, err := os.ReadFile(path)
	if err != nil || string(profile) != dist.Profile() {
		return errors.New("native child: shipped profile changed")
	}
	catalog, err := os.ReadFile(filepath.Join(home, "models.json"))
	if err != nil || !bytes.Equal(catalog, dist.Catalog()) {
		return errors.New("native child: shipped catalog changed")
	}
	oldLine, newLine := fmt.Sprintf("base_url = %q\n", original), fmt.Sprintf("base_url = %q\n", endpoint)
	if strings.Count(string(profile), oldLine) != 1 {
		return errors.New("native child: endpoint line is ambiguous")
	}
	if err := os.WriteFile(path, []byte(strings.Replace(string(profile), oldLine, newLine, 1)), 0600); err != nil {
		return err
	}
	profile, err = os.ReadFile(path)
	if err != nil || strings.Replace(string(profile), newLine, oldLine, 1) != dist.Profile() {
		return errors.New("native child: endpoint substitution changed profile")
	}
	catalog, err = os.ReadFile(filepath.Join(home, "models.json"))
	if err != nil || !bytes.Equal(catalog, dist.Catalog()) {
		return errors.New("native child: endpoint substitution changed catalog")
	}
	return nil
}

type shippedProfileKey struct{}

// WithShippedProfile selects the immutable shipped profile with only its local
// transport endpoint changed. The override is scoped to one controlled run.
func WithShippedProfile(ctx context.Context, endpoint string) (context.Context, error) {
	if err := validateEndpoint(endpoint); err != nil {
		return nil, err
	}
	return context.WithValue(ctx, shippedProfileKey{}, endpoint), nil
}

// ShippedEndpoint returns a validated endpoint explicitly scoped to this run.
func ShippedEndpoint(ctx context.Context) (string, bool) {
	endpoint, ok := ctx.Value(shippedProfileKey{}).(string)
	return endpoint, ok
}

func validateEndpoint(endpoint string) error {
	target, err := url.Parse(endpoint)
	if err != nil || target.Scheme != "http" || target.Path != "/v1" ||
		target.User != nil || target.RawQuery != "" || target.Fragment != "" ||
		target.Port() == "" || net.ParseIP(target.Hostname()) == nil || !net.ParseIP(target.Hostname()).IsLoopback() {
		return errors.New("native child: endpoint must be numeric loopback HTTP /v1")
	}
	return nil
}

// WriteShippedProfile copies the shipped source bytes into the isolated fixture.
func WriteShippedProfile(home, cwd string) error {
	if os.Mkdir(cwd, 0700) != nil || os.WriteFile(filepath.Join(home, "config.toml"), []byte(dist.Profile()), 0600) != nil || os.WriteFile(filepath.Join(home, "models.json"), dist.Catalog(), 0600) != nil {
		return errors.New("live: shipped profile copy failed")
	}
	return nil
}
