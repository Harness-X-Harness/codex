package live

import (
	"encoding/json"
	"io"
	"strings"
)

// parseStructuredReply validates one bounded task-result object. The caller
// owns its semantic facts and nullable-field policy; schema shape alone is not
// proof of a tool, file, search, history, or visual outcome.
func parseStructuredReply(reply string, out any, required ...string) error {
	invalid := func() error {
		return failedObservation("structured_reply_invalid", "live: structured task result is invalid")
	}
	if len(reply) == 0 || len(reply) > 4096 {
		return invalid()
	}
	tokens := json.NewDecoder(strings.NewReader(reply))
	if err := uniqueJSONValue(tokens, 0); err != nil {
		return invalid()
	}
	if _, err := tokens.Token(); err != io.EOF {
		return invalid()
	}
	var fields map[string]json.RawMessage
	if json.Unmarshal([]byte(reply), &fields) != nil || fields == nil || len(fields) != len(required) {
		return invalid()
	}
	for _, name := range required {
		if _, ok := fields[name]; !ok {
			return invalid()
		}
	}
	decoder := json.NewDecoder(strings.NewReader(reply))
	decoder.DisallowUnknownFields()
	if decoder.Decode(out) != nil {
		return invalid()
	}
	return nil
}

// Reject duplicate keys at every depth before decoding into Go structs, which
// would otherwise silently select the last value. No prefix/first-object repair.
func uniqueJSONValue(decoder *json.Decoder, depth int) error {
	if depth > 12 {
		return io.ErrUnexpectedEOF
	}
	token, err := decoder.Token()
	if err != nil {
		return err
	}
	delim, container := token.(json.Delim)
	if !container {
		return nil
	}
	switch delim {
	case '{':
		keys := map[string]bool{}
		for decoder.More() {
			key, err := decoder.Token()
			if err != nil {
				return err
			}
			name, ok := key.(string)
			if !ok || keys[name] || len(keys) >= 64 {
				return io.ErrUnexpectedEOF
			}
			keys[name] = true
			if err := uniqueJSONValue(decoder, depth+1); err != nil {
				return err
			}
		}
		end, err := decoder.Token()
		if err != nil {
			return err
		}
		if end != json.Delim('}') {
			return io.ErrUnexpectedEOF
		}
	case '[':
		count := 0
		for decoder.More() {
			count++
			if count > 64 {
				return io.ErrUnexpectedEOF
			}
			if err := uniqueJSONValue(decoder, depth+1); err != nil {
				return err
			}
		}
		end, err := decoder.Token()
		if err != nil {
			return err
		}
		if end != json.Delim(']') {
			return io.ErrUnexpectedEOF
		}
	default:
		return io.ErrUnexpectedEOF
	}
	return nil
}
