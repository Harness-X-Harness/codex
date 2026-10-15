package live

import "encoding/json"

// frame is the in-memory notification shape shared by scenario observers.
// It is not a second App Server client.
type frame struct {
	size   int
	ID     json.RawMessage `json:"id,omitempty"`
	Method string          `json:"method,omitempty"`
	Params json.RawMessage `json:"params,omitempty"`
	Result json.RawMessage `json:"result,omitempty"`
	Error  json.RawMessage `json:"error,omitempty"`
}
