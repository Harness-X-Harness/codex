package live

import (
	"bytes"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"image/png"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"time"
	"unicode/utf8"
)

const (
	diagnosticsMaxEvents        = 4096
	diagnosticsMaxEventBytes    = 64 << 10
	diagnosticsMaxArtifactBytes = 32 << 20
	diagnosticsMaxArtifacts     = 32
	diagnosticsMaxBytes         = 128 << 20
)

// Diagnostics is optional, task-scoped evidence. Capture failure never changes
// the business result. Call Close after the business result has been recorded.
type Diagnostics struct {
	mu                  sync.Mutex
	root                *os.Root
	events              *os.File
	secrets             []string
	status              DiagnosticsStatus
	maxEvents, maxBytes int
	manifestOwned       bool
	artifactNames       map[string]string
	artifactDigests     map[string]string
}

type DiagnosticsStatus struct {
	Enabled       bool     `json:"enabled"`
	Closed        bool     `json:"closed"`
	Events        int      `json:"events"`
	Artifacts     int      `json:"artifacts"`
	Bytes         int      `json:"bytes"`
	Truncated     bool     `json:"truncated"`
	CaptureErrors []string `json:"capture_errors,omitempty"`
}

func NewDiagnostics(dir string, secrets ...string) *Diagnostics {
	d := &Diagnostics{maxEvents: diagnosticsMaxEvents, maxBytes: diagnosticsMaxBytes, artifactNames: map[string]string{}, artifactDigests: map[string]string{}}
	for _, secret := range secrets {
		if secret != "" {
			d.secrets = append(d.secrets, secret)
		}
	}
	if dir == "" {
		return d
	}
	d.status.Enabled = true
	// A caller-owned directory may exist, but existing evidence is never replaced.
	if err := os.MkdirAll(dir, 0700); err != nil {
		d.captureError("directory_unavailable")
		return d
	}
	info, err := os.Lstat(dir)
	if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		d.captureError("unsafe_directory")
		return d
	}
	d.root, err = os.OpenRoot(dir)
	if err != nil {
		d.captureError("directory_unavailable")
		return d
	}
	d.events, err = d.root.OpenFile("events.jsonl", os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		d.captureError("events_unavailable")
	}
	d.initManifest()
	return d
}

func (d *Diagnostics) captureError(code string) {
	for _, prior := range d.status.CaptureErrors {
		if prior == code {
			return
		}
	}
	d.status.CaptureErrors = append(d.status.CaptureErrors, code)
}
func (d *Diagnostics) limit(code string) { d.status.Truncated = true; d.captureError(code) }

// Record writes an event immediately, without retaining raw protocol traffic.
func (d *Diagnostics) Record(kind string, fields map[string]any) {
	if d == nil {
		return
	}
	d.mu.Lock()
	defer d.mu.Unlock()
	defer d.persistStatus()
	if !d.status.Enabled {
		return
	}
	if d.status.Closed {
		d.captureError("record_after_close")
		return
	}
	if d.events == nil {
		d.captureError("events_unavailable")
		return
	}
	if d.status.Events >= d.maxEvents {
		d.limit("event_limit")
		return
	}
	cleaned := d.clean(fields, 0)
	if len(kind) > 128 {
		d.limit("kind_size_limit")
		kind = kind[:128]
	}
	event := map[string]any{"kind": d.cleanString(kind), "at": time.Now().UTC().Format(time.RFC3339Nano), "fields": cleaned}
	data, err := json.Marshal(event)
	if err != nil {
		d.captureError("event_encoding_failed")
		return
	}
	if len(data) > diagnosticsMaxEventBytes {
		d.limit("event_size_limit")
		data, _ = json.Marshal(map[string]any{"kind": d.cleanString(kind), "truncated": true, "original_bytes": len(data)})
	}
	data = append(data, '\n')
	if d.status.Bytes+len(data) > d.maxBytes {
		d.limit("total_byte_limit")
		return
	}
	n, err := d.events.Write(data)
	d.status.Bytes += n
	if err != nil || n != len(data) {
		d.captureError("event_write_failed")
		return
	}
	d.status.Events++
}

var diagnosticName = regexp.MustCompile(`^[a-z][a-z0-9_-]{0,95}\.(png|txt|diff)$`)

// Artifact preserves PNG pixels, or sanitized task text, under a fixed basename.
// It returns no path on failure; the manifest records capture failure separately.
func (d *Diagnostics) Artifact(name string, data []byte) string {
	if d == nil {
		return ""
	}
	d.mu.Lock()
	defer d.mu.Unlock()
	defer d.persistStatus()
	if !d.status.Enabled {
		return ""
	}
	if d.status.Closed {
		d.captureError("artifact_after_close")
		return ""
	}
	if d.root == nil {
		d.captureError("directory_unavailable")
		return ""
	}
	if !diagnosticName.MatchString(name) || filepath.Base(name) != name {
		d.captureError("unsafe_artifact_name")
		return ""
	}
	if len(data) > diagnosticsMaxArtifactBytes {
		d.limit("artifact_size_limit")
		return ""
	}
	if strings.HasSuffix(name, ".png") {
		cfg, err := png.DecodeConfig(bytes.NewReader(data))
		if err != nil || cfg.Width <= 0 || cfg.Height <= 0 || int64(cfg.Width)*int64(cfg.Height) > diagnosticsMaxArtifactBytes/8 {
			d.captureError("invalid_png_artifact")
			return ""
		}
		if _, err := png.Decode(bytes.NewReader(data)); err != nil {
			d.captureError("invalid_png_artifact")
			return ""
		}
		for _, secret := range d.secrets {
			if bytes.Contains(data, []byte(secret)) {
				d.captureError("artifact_contains_secret")
				return ""
			}
		}
	} else {
		if !utf8.Valid(data) {
			d.captureError("invalid_text_artifact")
			return ""
		}
		data = []byte(d.cleanString(string(data)))
	}
	digestBytes := sha256.Sum256(data)
	digest := hex.EncodeToString(digestBytes[:])
	if prior, exists := d.artifactNames[name]; exists && prior != digest {
		d.captureError("artifact_name_conflict")
		return ""
	}
	if prior, exists := d.artifactDigests[digest]; exists {
		return prior
	}
	if d.status.Artifacts >= diagnosticsMaxArtifacts {
		d.limit("artifact_count_limit")
		return ""
	}
	if len(data) > diagnosticsMaxArtifactBytes || d.status.Bytes+len(data) > d.maxBytes {
		d.limit("total_byte_limit")
		return ""
	}
	file, err := d.root.OpenFile(name, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		d.captureError("artifact_unavailable")
		return ""
	}
	n, err := file.Write(data)
	d.status.Bytes += n
	closeErr := file.Close()
	if err != nil || n != len(data) || closeErr != nil {
		d.captureError("artifact_write_failed")
		return ""
	}
	d.status.Artifacts++
	d.artifactNames[name] = digest
	d.artifactDigests[digest] = name
	return name
}

func (d *Diagnostics) Status() DiagnosticsStatus {
	if d == nil {
		return DiagnosticsStatus{}
	}
	d.mu.Lock()
	defer d.mu.Unlock()
	result := d.status
	result.CaptureErrors = append([]string(nil), result.CaptureErrors...)
	return result
}

func (d *Diagnostics) Close() {
	if d == nil {
		return
	}
	d.mu.Lock()
	defer d.mu.Unlock()
	if d.status.Closed {
		return
	}
	d.status.Closed = true
	if d.events != nil && d.events.Close() != nil {
		d.captureError("events_close_failed")
	}
	if d.root == nil {
		return
	}
	d.persistStatus()
	if d.root.Close() != nil {
		d.captureError("directory_close_failed")
	}
}

// An initial open manifest survives interrupted collection. Status updates are
// atomic and independent of evidence limits; they never overwrite another run.
func (d *Diagnostics) initManifest() {
	if d.root == nil {
		return
	}
	file, err := d.root.OpenFile("manifest.json", os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		d.captureError("manifest_unavailable")
		return
	}
	d.manifestOwned = true
	data, _ := json.MarshalIndent(d.status, "", "  ")
	if n, err := file.Write(append(data, '\n')); err != nil || n != len(data)+1 {
		d.captureError("manifest_write_failed")
	}
	if file.Close() != nil {
		d.captureError("manifest_write_failed")
	}
}
func (d *Diagnostics) persistStatus() {
	if d.root == nil || !d.manifestOwned {
		return
	}
	file, err := d.root.OpenFile("manifest.pending", os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		d.captureError("manifest_unavailable")
		return
	}
	data, _ := json.MarshalIndent(d.status, "", "  ")
	n, writeErr := file.Write(append(data, '\n'))
	closeErr := file.Close()
	if writeErr != nil || n != len(data)+1 || closeErr != nil {
		d.captureError("manifest_write_failed")
		_ = d.root.Remove("manifest.pending")
		return
	}
	if d.root.Rename("manifest.pending", "manifest.json") != nil {
		d.captureError("manifest_write_failed")
		_ = d.root.Remove("manifest.pending")
	}
}

func sensitiveDiagnosticField(key string) bool {
	key = strings.ToLower(strings.NewReplacer("_", "", "-", "", ".", "").Replace(key))
	switch key {
	case "defaultreasoningeffort", "supportedreasoningefforts", "reasoningeffort", "reasoningefforts", "reasoninglevel", "supportsreasoning", "reasoningenabled":
		return false
	}
	for _, fragment := range []string{"authorization", "password", "secret", "cookie", "apikey", "accesstoken", "refreshtoken", "cipher", "encrypted", "reasoning", "privatekey", "credential"} {
		if strings.Contains(key, fragment) {
			return true
		}
	}
	return key == "key" || key == "auth" || key == "env" || key == "envvars" || key == "environmentvariables" || strings.HasPrefix(key, "raw")
}

var diagnosticCookie = regexp.MustCompile(`(?im)\b(?:authorization|proxy-authorization|set-cookie|cookie)\s*[:=]\s*[^\r\n]+`)
var diagnosticBearer = regexp.MustCompile(`(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]+`)
var diagnosticAssignment = regexp.MustCompile(`(?i)\b(api[_-]?key|access[_-]?token|refresh[_-]?token|password|secret|authorization|cookie)["']?\s*[:=]\s*(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\s"',;]+)`)
var diagnosticFlag = regexp.MustCompile(`(?i)(--(?:api[-_]?key|password|token|secret))\s+(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\s"',;]+)`)
var diagnosticURL = regexp.MustCompile(`(?i)https?://[^\s"<>]+`)
var diagnosticKey = regexp.MustCompile(`\b(?:xai-|sk-)[A-Za-z0-9_-]{16,}\b`)

func (d *Diagnostics) cleanString(value string) string {
	for _, secret := range d.secrets {
		value = strings.ReplaceAll(value, secret, "[REDACTED]")
	}
	value = diagnosticCookie.ReplaceAllString(value, "Cookie: [REDACTED]")
	value = diagnosticBearer.ReplaceAllString(value, "[REDACTED_AUTH]")
	value = diagnosticAssignment.ReplaceAllString(value, "$1=[REDACTED]")
	value = diagnosticFlag.ReplaceAllString(value, "$1 [REDACTED]")
	value = diagnosticKey.ReplaceAllString(value, "[REDACTED_KEY]")
	value = diagnosticURL.ReplaceAllStringFunc(value, func(raw string) string {
		parsed, err := url.Parse(raw)
		if err != nil {
			return "[INVALID_URL]"
		}
		if parsed.User != nil {
			parsed.User = url.User("REDACTED")
		}
		query := parsed.Query()
		for key := range query {
			if strings.EqualFold(key, "sig") || strings.EqualFold(key, "code") || strings.EqualFold(key, "token") || sensitiveDiagnosticField(key) || strings.Contains(strings.ToLower(key), "signature") || strings.Contains(strings.ToLower(key), "credential") {
				query.Set(key, "REDACTED")
			}
		}
		parsed.RawQuery = query.Encode()
		return parsed.String()
	})
	if strings.HasPrefix(value, "data:image/") {
		return "[IMAGE_DATA_OMITTED]"
	}
	return value
}

func (d *Diagnostics) clean(value any, depth int) any {
	if depth > 12 {
		d.limit("field_depth_limit")
		return "[TRUNCATED]"
	}
	switch value := value.(type) {
	case string:
		return d.cleanString(value)
	case map[string]any:
		result := map[string]any{}
		for key, field := range value {
			if !sensitiveDiagnosticField(key) {
				result[d.cleanString(key)] = d.clean(field, depth+1)
			}
		}
		return result
	case []any:
		result := make([]any, 0, len(value))
		for _, field := range value {
			result = append(result, d.clean(field, depth+1))
		}
		return result
	case json.RawMessage:
		var decoded any
		if json.Unmarshal(value, &decoded) != nil {
			d.captureError("field_encoding_failed")
			return "[UNAVAILABLE]"
		}
		return d.clean(decoded, depth+1)
	case []byte:
		return "[BINARY_OMITTED]"
	case nil, bool, float64, float32, int, int64, int32, uint, uint64, uint32, json.Number:
		return value
	default:
		data, err := json.Marshal(value)
		if err != nil {
			d.captureError("field_encoding_failed")
			return "[UNAVAILABLE]"
		}
		var decoded any
		if json.Unmarshal(data, &decoded) != nil {
			d.captureError("field_encoding_failed")
			return "[UNAVAILABLE]"
		}
		return d.clean(decoded, depth+1)
	}
}

// Item is a projection of public App Server items, never a raw-event subscriber.
func (d *Diagnostics) noteError(code string) {
	if d == nil {
		return
	}
	d.mu.Lock()
	defer d.mu.Unlock()
	d.captureError(code)
	d.persistStatus()
}

func (d *Diagnostics) Item(source string, raw json.RawMessage) {
	if d == nil {
		return
	}
	var item map[string]any
	if json.Unmarshal(raw, &item) != nil {
		d.Record("item_unavailable", map[string]any{"source": source})
		return
	}
	kind, _ := item["type"].(string)
	switch kind {
	case "agentMessage", "userMessage", "commandExecution", "fileChange", "mcpToolCall", "dynamicToolCall", "collabAgentToolCall", "collabAgentSpawn", "collabAgentInteraction", "collabAgentResume", "collabAgentClose", "collabAgentWait", "subAgentActivity", "webSearch":
	case "imageGeneration":
		if result, ok := item["result"].(string); ok {
			item["payloadBytes"] = len(result)
			if result != "" {
				if len(result) > base64.StdEncoding.EncodedLen(diagnosticsMaxArtifactBytes) {
					d.noteError("image_payload_limit")
				} else if pixels, err := base64.StdEncoding.Strict().DecodeString(result); err != nil {
					d.noteError("image_payload_invalid")
				} else {
					digest := sha256.Sum256(pixels)
					if reference := d.Artifact("image-"+hex.EncodeToString(digest[:])+".png", pixels); reference != "" {
						item["artifact"] = reference
					}
				}
			}
		}
		delete(item, "result")
	case "reasoning":
		item = map[string]any{"type": kind, "id": item["id"], "status": item["status"], "payloadBytes": len(raw)}
	default:
		item = map[string]any{"type": kind, "id": item["id"], "status": item["status"], "projection": "metadata_only"}
	}
	d.Record("public_item", map[string]any{"source": source, "item": item})
}
