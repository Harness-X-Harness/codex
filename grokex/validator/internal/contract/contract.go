// Package contract reads grokex/live_contracts.json, the executable Live
// contract that names each scenario's Story and Turn deadline.
package contract

import (
	"encoding/json"
	"fmt"
	"os"
	"time"
)

// Scenario is one entry of the contract's "scenarios" object.
type Scenario struct {
	Story               string `json:"story"`
	TurnDeadlineSeconds int    `json:"turn_deadline_seconds"`
}

// TurnDeadline is the bound for one Turn of the scenario.
func (s Scenario) TurnDeadline() time.Duration {
	return time.Duration(s.TurnDeadlineSeconds) * time.Second
}

// Contract is the parsed live_contracts.json.
type Contract struct {
	SchemaVersion int                 `json:"schema_version"`
	Scenarios     map[string]Scenario `json:"scenarios"`
}

// Load parses the contract at path.
func Load(path string) (Contract, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return Contract{}, fmt.Errorf("read live contract: %w", err)
	}
	var contract Contract
	if err := json.Unmarshal(data, &contract); err != nil {
		return Contract{}, fmt.Errorf("parse live contract: %w", err)
	}
	if len(contract.Scenarios) == 0 {
		return Contract{}, fmt.Errorf("live contract %s names no scenarios", path)
	}
	return contract, nil
}

// Scenario returns the named scenario or an error naming the contract gap.
func (c Contract) Scenario(name string) (Scenario, error) {
	scenario, ok := c.Scenarios[name]
	if !ok {
		return Scenario{}, fmt.Errorf("scenario %q is not in the live contract", name)
	}
	if scenario.TurnDeadlineSeconds <= 0 {
		return Scenario{}, fmt.Errorf("scenario %q has no Turn deadline", name)
	}
	if scenario.Story == "" {
		return Scenario{}, fmt.Errorf("scenario %q names no Story", name)
	}
	return scenario, nil
}
