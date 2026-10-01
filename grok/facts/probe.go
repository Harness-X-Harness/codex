// Package facts records bounded backend observations, independently of product Live.
package facts

import (
	"context"
	"errors"
)

// Observation contains only safe metadata; it never contains backend output.
type Observation struct {
	Requests             int
	HTTPStatus           int
	Stage                string
	Completed            bool
	TextBytes            int
	EncryptedItems       int
	Replayed             bool
	ReturnedModelMatches bool
}

// Probe observes one explicitly selected HTTP Responses endpoint.
type Probe struct{}

func NewProbe(endpoint, apiKey string) (*Probe, error) {
	return &Probe{}, nil
}

func (p *Probe) Text(ctx context.Context, model string) (Observation, error) {
	return Observation{}, errors.New("text observation not implemented")
}
