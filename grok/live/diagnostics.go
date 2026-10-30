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
	diagnosticsQueueSize        = 128
	diagnosticsFlushTimeout     = 2 * time.Second
	diagnosticsMaxErrors        = 32
)

type diagnosticWrite struct {
	name string // Empty for an event; otherwise a validated artifact basename.
	data []byte // Immutable, sanitized before admission.
}

// Tests use these cut points to hold filesystem work without holding mu.
type diagnosticWriterHooks struct {
	beforeWrite    func(string)
	beforeManifest func(DiagnosticsStatus)
	flushTimeout   time.Duration
}

// Diagnostics is optional, task-scoped evidence. Capture failure never changes
// the business result. Record and Artifact do bounded admission, not filesystem
// I/O. Their caller-owned inputs must not be mutated concurrently with the call.
// Call Close after recording the business result; Close bounds its flush wait.
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
	queue               chan diagnosticWrite
	wake                chan struct{}
	stop                chan struct{}
	done                chan struct{}
	hooks               diagnosticWriterHooks
	admittedEvents      int
	admittedBytes       int
	flushSettled        bool
}

type DiagnosticsStatus struct {
	Enabled          bool     `json:"enabled"`
	Closed           bool     `json:"closed"`
	Events           int      `json:"events"`
	Artifacts        int      `json:"artifacts"`
	Bytes            int      `json:"bytes"`
	PendingWrites    int      `json:"pending_writes"`
	DroppedEvents    int      `json:"dropped_events"`
	DroppedArtifacts int      `json:"dropped_artifacts"`
	FlushTimedOut    bool     `json:"flush_timed_out"`
	Truncated        bool     `json:"truncated"`
	CaptureErrors    []string `json:"capture_errors,omitempty"`
}

func NewDiagnostics(dir string, secrets ...string) *Diagnostics {
	return newDiagnostics(dir, diagnosticWriterHooks{}, secrets...)
}

func newDiagnostics(dir string, hooks diagnosticWriterHooks, secrets ...string) *Diagnostics {
	if hooks.flushTimeout <= 0 {
		hooks.flushTimeout = diagnosticsFlushTimeout
	}
	d := &Diagnostics{
		maxEvents: diagnosticsMaxEvents, maxBytes: diagnosticsMaxBytes,
		artifactNames: map[string]string{}, artifactDigests: map[string]string{},
		queue: make(chan diagnosticWrite, diagnosticsQueueSize), wake: make(chan struct{}, 1),
		stop: make(chan struct{}), done: make(chan struct{}), hooks: hooks,
	}
	for _, secret := range secrets {
		if secret != "" {
			d.secrets = append(d.secrets, secret)
		}
	}
	started := false
	defer func() {
		if !started {
			close(d.done)
		}
	}()
	if dir == "" {
		return d
	}
	d.status.Enabled = true
	// Initialization may perform I/O. Existing evidence is never replaced.
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
	started = true
	go d.writeLoop()
	return d
}

// The caller holds mu, except during initialization before publication.
func (d *Diagnostics) captureError(code string) {
	for _, prior := range d.status.CaptureErrors {
		if prior == code {
			return
		}
	}
	if len(d.status.CaptureErrors) < diagnosticsMaxErrors {
		d.status.CaptureErrors = append(d.status.CaptureErrors, code)
	}
}

func (d *Diagnostics) limit(code string) { d.status.Truncated = true; d.captureError(code) }

func incrementDiagnosticCount(value *int) {
	if *value < int(^uint(0)>>1) {
		*value++
	}
}

func (d *Diagnostics) changed() {
	select {
	case d.wake <- struct{}{}:
	default:
	}
}

func (d *Diagnostics) drop(artifact bool, code string, truncated bool) {
	if artifact {
		incrementDiagnosticCount(&d.status.DroppedArtifacts)
	} else {
		incrementDiagnosticCount(&d.status.DroppedEvents)
	}
	if truncated {
		d.limit(code)
	} else {
		d.captureError(code)
	}
}

// Record snapshots a sanitized event, then tries the bounded queue once. It
// never waits for a queue slot or a filesystem write. Counts describe written
// evidence, not merely admitted jobs.
func (d *Diagnostics) Record(kind string, fields map[string]any) {
	if d == nil {
		return
	}
	d.mu.Lock()
	defer d.mu.Unlock()
	defer d.changed()
	if !d.status.Enabled {
		return
	}
	if d.status.Closed {
		d.drop(false, "record_after_close", false)
		return
	}
	if d.events == nil {
		d.drop(false, "events_unavailable", false)
		return
	}
	if d.admittedEvents >= d.maxEvents {
		d.drop(false, "event_limit", true)
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
		d.drop(false, "event_encoding_failed", false)
		return
	}
	if len(data) > diagnosticsMaxEventBytes {
		d.limit("event_size_limit")
		data, _ = json.Marshal(map[string]any{"kind": d.cleanString(kind), "truncated": true, "original_bytes": len(data)})
	}
	data = append(data, '\n')
	if d.admittedBytes+len(data) > d.maxBytes {
		d.drop(false, "total_byte_limit", true)
		return
	}
	select {
	case d.queue <- diagnosticWrite{data: data}:
		d.admittedEvents++
		d.admittedBytes += len(data)
		d.status.PendingWrites++
	default:
		d.drop(false, "queue_full", true)
	}
}

var diagnosticName = regexp.MustCompile(`^[a-z][a-z0-9_-]{0,95}\.(png|txt|diff)$`)

// Artifact queues an immutable PNG or sanitized task text under a fixed
// basename. A returned reference means admitted, not durably written; Status
// and the manifest report asynchronous capture failure separately.
func (d *Diagnostics) Artifact(name string, data []byte) string {
	if d == nil {
		return ""
	}
	d.mu.Lock()
	defer d.mu.Unlock()
	defer d.changed()
	if !d.status.Enabled {
		return ""
	}
	if d.status.Closed {
		d.drop(true, "artifact_after_close", false)
		return ""
	}
	if d.root == nil {
		d.drop(true, "directory_unavailable", false)
		return ""
	}
	if !diagnosticName.MatchString(name) || filepath.Base(name) != name {
		d.drop(true, "unsafe_artifact_name", false)
		return ""
	}
	if len(data) > diagnosticsMaxArtifactBytes {
		d.drop(true, "artifact_size_limit", true)
		return ""
	}
	if strings.HasSuffix(name, ".png") {
		cfg, err := png.DecodeConfig(bytes.NewReader(data))
		if err != nil || cfg.Width <= 0 || cfg.Height <= 0 || int64(cfg.Width)*int64(cfg.Height) > diagnosticsMaxArtifactBytes/8 {
			d.drop(true, "invalid_png_artifact", false)
			return ""
		}
		if _, err := png.Decode(bytes.NewReader(data)); err != nil {
			d.drop(true, "invalid_png_artifact", false)
			return ""
		}
		for _, secret := range d.secrets {
			if bytes.Contains(data, []byte(secret)) {
				d.drop(true, "artifact_contains_secret", false)
				return ""
			}
		}
		data = bytes.Clone(data)
	} else {
		if !utf8.Valid(data) {
			d.drop(true, "invalid_text_artifact", false)
			return ""
		}
		data = []byte(d.cleanString(string(data)))
	}
	digestBytes := sha256.Sum256(data)
	digest := hex.EncodeToString(digestBytes[:])
	if prior, exists := d.artifactNames[name]; exists && prior != digest {
		d.drop(true, "artifact_name_conflict", false)
		return ""
	}
	if prior, exists := d.artifactDigests[digest]; exists {
		return prior
	}
	if len(d.artifactNames) >= diagnosticsMaxArtifacts {
		d.drop(true, "artifact_count_limit", true)
		return ""
	}
	if len(data) > diagnosticsMaxArtifactBytes || d.admittedBytes+len(data) > d.maxBytes {
		d.drop(true, "total_byte_limit", true)
		return ""
	}
	select {
	case d.queue <- diagnosticWrite{name: name, data: data}:
		d.admittedBytes += len(data)
		d.status.PendingWrites++
		d.artifactNames[name] = digest
		d.artifactDigests[digest] = name
		return name
	default:
		d.drop(true, "queue_full", true)
		return ""
	}
}

func (d *Diagnostics) Status() DiagnosticsStatus {
	if d == nil {
		return DiagnosticsStatus{}
	}
	d.mu.Lock()
	defer d.mu.Unlock()
	return d.snapshot()
}

func (d *Diagnostics) snapshot() DiagnosticsStatus {
	result := d.status
	result.CaptureErrors = append([]string(nil), result.CaptureErrors...)
	return result
}

func (d *Diagnostics) Close() {
	if d == nil {
		return
	}
	d.mu.Lock()
	if !d.status.Closed {
		d.status.Closed = true
		close(d.stop)
	}
	d.mu.Unlock()
	timer := time.NewTimer(d.hooks.flushTimeout)
	defer timer.Stop()
	select {
	case <-d.done:
	case <-timer.C:
		d.mu.Lock()
		// Prefer a completed drain if the timer and completion became ready
		// together. Once final persistence settles, only handle cleanup remains.
		if !d.flushSettled {
			select {
			case <-d.done:
			default:
				d.status.FlushTimedOut = true
				d.captureError("flush_timeout")
			}
		}
		d.mu.Unlock()
	}
}

func (d *Diagnostics) writeLoop() {
	defer close(d.done)
	for {
		select {
		case job := <-d.queue:
			d.write(job)
			d.persistStatus()
		case <-d.wake:
			d.persistStatus()
		case <-d.stop:
			// Admission stopped under mu before stop closed. No sender can add
			// a job after this finite drain.
			for {
				select {
				case job := <-d.queue:
					d.write(job)
				default:
					d.finish()
					return
				}
			}
		}
	}
}

func (d *Diagnostics) write(job diagnosticWrite) {
	if d.hooks.beforeWrite != nil {
		d.hooks.beforeWrite(job.name)
	}
	var n int
	var code string
	if job.name == "" {
		var err error
		n, err = d.events.Write(job.data)
		if err != nil || n != len(job.data) {
			code = "event_write_failed"
		}
	} else {
		file, err := d.root.OpenFile(job.name, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
		if err != nil {
			code = "artifact_unavailable"
		} else {
			n, err = file.Write(job.data)
			closeErr := file.Close()
			if err != nil || n != len(job.data) || closeErr != nil {
				code = "artifact_write_failed"
			}
		}
	}
	d.mu.Lock()
	d.status.Bytes += n
	d.status.PendingWrites--
	if code != "" {
		d.captureError(code)
	} else if job.name == "" {
		d.status.Events++
	} else {
		d.status.Artifacts++
	}
	d.mu.Unlock()
}

func (d *Diagnostics) finish() {
	if d.events != nil && d.events.Close() != nil {
		d.mu.Lock()
		d.captureError("events_close_failed")
		d.mu.Unlock()
	}
	// A timeout may occur while the final manifest write is blocked. Republish
	// once if that sticky bit changed; do not loop on unbounded late Record calls.
	for {
		written := d.persistStatus()
		d.mu.Lock()
		if d.status.FlushTimedOut && !written.FlushTimedOut {
			d.mu.Unlock()
			continue
		}
		d.flushSettled = true
		d.mu.Unlock()
		break
	}
	if d.root.Close() != nil {
		d.mu.Lock()
		d.captureError("directory_close_failed")
		d.mu.Unlock()
	}
}

// An initial open manifest survives interruption. The single writer alone
// replaces owned manifests; callers never do filesystem work while holding mu.
func (d *Diagnostics) initManifest() {
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

func (d *Diagnostics) persistStatus() DiagnosticsStatus {
	snapshot := d.Status()
	if d.root == nil || !d.manifestOwned {
		return snapshot
	}
	if d.hooks.beforeManifest != nil {
		d.hooks.beforeManifest(snapshot)
	}
	file, err := d.root.OpenFile("manifest.pending", os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	code := ""
	if err != nil {
		code = "manifest_unavailable"
	} else {
		data, _ := json.MarshalIndent(snapshot, "", "  ")
		n, writeErr := file.Write(append(data, '\n'))
		closeErr := file.Close()
		if writeErr != nil || n != len(data)+1 || closeErr != nil || d.root.Rename("manifest.pending", "manifest.json") != nil {
			code = "manifest_write_failed"
			_ = d.root.Remove("manifest.pending")
		}
	}
	if code != "" {
		d.mu.Lock()
		d.captureError(code)
		d.mu.Unlock()
	}
	return snapshot
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
	d.changed()
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
