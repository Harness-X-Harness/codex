// Package dist exposes the shipped source assets to their Facts and Live consumers.
package dist

import (
	_ "embed"
	"encoding/json"
	"errors"
	"regexp"
)

//go:embed config.toml.example
var profile string

//go:embed models.json
var catalog []byte

// Profile returns the exact shipped configuration bytes.
func Profile() string { return profile }

// Catalog returns an independent copy of the exact shipped catalog bytes.
func Catalog() []byte { return append([]byte(nil), catalog...) }

// ModelSlugs preserves the shipped order; it never consumes remote observations.
func ModelSlugs() ([]string, error) {
	var value struct {
		Models []struct {
			Slug string `json:"slug"`
		} `json:"models"`
	}
	if json.Unmarshal(catalog, &value) != nil || len(value.Models) == 0 {
		return nil, errors.New("invalid shipped catalog")
	}
	slugs := make([]string, 0, len(value.Models))
	seen := make(map[string]bool)
	for _, model := range value.Models {
		if model.Slug == "" || seen[model.Slug] {
			return nil, errors.New("invalid shipped model identity")
		}
		seen[model.Slug] = true
		slugs = append(slugs, model.Slug)
	}
	return slugs, nil
}

// Defaults reads the top-level model and endpoint from this owned profile format.
func Defaults() (model, baseURL string, err error) {
	selected := regexp.MustCompile(`(?m)^model = "([^"]+)"$`).FindStringSubmatch(profile)
	endpoint := regexp.MustCompile(`(?m)^base_url = "([^"]+)"$`).FindStringSubmatch(profile)
	slugs, parseErr := ModelSlugs()
	if len(selected) != 2 || len(endpoint) != 2 || parseErr != nil {
		return "", "", errors.New("invalid shipped profile")
	}
	for _, slug := range slugs {
		if slug == selected[1] {
			return selected[1], endpoint[1], nil
		}
	}
	return "", "", errors.New("shipped default absent from catalog")
}
