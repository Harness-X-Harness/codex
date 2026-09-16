package live

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"maps"
	"net"
	"net/http"
	"net/http/httptest"
	"net/http/httputil"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"testing"
)

const (
	wireRingSize        = 8
	wireRequestBodyCap  = 4 << 20
	wireResponseBodyCap = 64 << 10
)

type wireExchangeContextKey struct{}

type wireExchange struct {
	method           string
	path             string
	requestBody      []byte
	requestTruncated bool
	status           int
	responseBody     []byte
}

type wireRecorder struct {
	mu              sync.Mutex
	exchanges       []wireExchange
	ringSize        int
	requestBodyCap  int
	responseBodyCap int
	proxy           *httputil.ReverseProxy
}

func newWireRecorder() *wireRecorder {
	return &wireRecorder{
		ringSize:        wireRingSize,
		requestBodyCap:  wireRequestBodyCap,
		responseBodyCap: wireResponseBodyCap,
	}
}

func startWireProxy(t *testing.T, upstreamRaw string) (*httptest.Server, *wireRecorder, string) {
	t.Helper()
	upstream, err := url.Parse(upstreamRaw)
	if err != nil || upstream.Scheme == "" || upstream.Host == "" {
		t.Fatalf("parse upstream base_url %q: %v", upstreamRaw, err)
	}
	rec := newWireRecorder()
	rec.proxy = &httputil.ReverseProxy{
		Director: func(req *http.Request) {
			req.URL.Scheme = upstream.Scheme
			req.URL.Host = upstream.Host
			req.Host = upstream.Host
		},
		FlushInterval:  -1,
		ModifyResponse: rec.modifyResponse,
		// ModifyResponse never runs when the upstream is unreachable; record the
		// transport failure as the exchange's status so RED output does not show 0.
		ErrorHandler: func(w http.ResponseWriter, req *http.Request, err error) {
			if ex, _ := req.Context().Value(wireExchangeContextKey{}).(*wireExchange); ex != nil {
				ex.status = http.StatusBadGateway
				ex.responseBody = []byte("wire proxy: " + err.Error())
			}
			w.WriteHeader(http.StatusBadGateway)
		},
	}
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen wire proxy: %v", err)
	}
	server := httptest.NewUnstartedServer(rec)
	if server.Listener != nil {
		_ = server.Listener.Close()
	}
	server.Listener = ln
	server.Start()
	proxyBase := strings.TrimRight(server.URL, "/") + upstream.Path
	return server, rec, proxyBase
}

func (r *wireRecorder) ServeHTTP(w http.ResponseWriter, req *http.Request) {
	ex := r.captureRequest(req)
	req = req.WithContext(context.WithValue(req.Context(), wireExchangeContextKey{}, ex))
	r.proxy.ServeHTTP(w, req)
	r.add(*ex)
}

func (r *wireRecorder) captureRequest(req *http.Request) *wireExchange {
	capn := r.requestBodyCap
	if capn <= 0 {
		capn = wireRequestBodyCap
	}
	var body []byte
	truncated := false
	if req.Body != nil {
		var err error
		body, truncated, err = readCapped(req.Body, capn)
		_ = req.Body.Close()
		if err != nil && body == nil {
			body = []byte{}
		}
	}
	req.Body = io.NopCloser(bytes.NewReader(body))
	req.GetBody = func() (io.ReadCloser, error) {
		return io.NopCloser(bytes.NewReader(body)), nil
	}
	req.ContentLength = int64(len(body))
	req.Header.Set("Content-Length", strconv.Itoa(len(body)))
	req.Header.Del("Transfer-Encoding")
	req.TransferEncoding = nil
	path := req.URL.Path
	if req.URL.RawQuery != "" {
		path += "?" + req.URL.RawQuery
	}
	return &wireExchange{
		method:           req.Method,
		path:             path,
		requestBody:      body,
		requestTruncated: truncated,
	}
}

func (r *wireRecorder) modifyResponse(resp *http.Response) error {
	ex, _ := resp.Request.Context().Value(wireExchangeContextKey{}).(*wireExchange)
	if ex != nil {
		ex.status = resp.StatusCode
	}
	if resp.StatusCode >= 200 && resp.StatusCode <= 299 {
		return nil
	}
	capn := r.responseBodyCap
	if capn <= 0 {
		capn = wireResponseBodyCap
	}
	if resp.Body == nil {
		return nil
	}
	body, _, err := readCapped(resp.Body, capn)
	_ = resp.Body.Close()
	if err != nil && body == nil {
		body = []byte{}
	}
	resp.Body = io.NopCloser(bytes.NewReader(body))
	resp.ContentLength = int64(len(body))
	resp.Header.Set("Content-Length", strconv.Itoa(len(body)))
	resp.TransferEncoding = nil
	if ex != nil {
		ex.responseBody = body
	}
	return nil
}

func (r *wireRecorder) add(ex wireExchange) {
	r.mu.Lock()
	defer r.mu.Unlock()
	size := r.ringSize
	if size <= 0 {
		size = wireRingSize
	}
	r.exchanges = append(r.exchanges, ex)
	if len(r.exchanges) > size {
		keep := r.exchanges[len(r.exchanges)-size:]
		next := make([]wireExchange, len(keep))
		copy(next, keep)
		r.exchanges = next
	}
}

func (r *wireRecorder) snapshot() []wireExchange {
	if r == nil {
		return nil
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	out := make([]wireExchange, len(r.exchanges))
	copy(out, r.exchanges)
	return out
}

func (r *wireRecorder) non2xx() []wireExchange {
	var out []wireExchange
	for _, ex := range r.snapshot() {
		if ex.status < 200 || ex.status > 299 {
			out = append(out, ex)
		}
	}
	return out
}

func readCapped(r io.Reader, limit int) ([]byte, bool, error) {
	if r == nil || limit < 0 {
		return nil, false, nil
	}
	data, err := io.ReadAll(io.LimitReader(r, int64(limit)+1))
	if len(data) > limit {
		return data[:limit], true, err
	}
	return data, false, err
}

func shape(body []byte) map[string]string {
	var value any
	if err := json.Unmarshal(body, &value); err != nil {
		return map[string]string{"_": "non-json"}
	}
	out := map[string]string{}
	walkShape("", value, out)
	return out
}

func walkShape(prefix string, value any, out map[string]string) {
	switch typed := value.(type) {
	case map[string]any:
		if len(typed) == 0 {
			out[shapeKey(prefix)] = "object"
			return
		}
		for key, child := range typed {
			next := key
			if prefix != "" {
				next = prefix + "." + key
			}
			walkShape(next, child, out)
		}
	case []any:
		if len(typed) == 0 {
			out[shapeKey(prefix)] = "array"
			return
		}
		for i, child := range typed {
			walkShape(fmt.Sprintf("%s[%d]", prefix, i), child, out)
		}
	case string:
		out[shapeKey(prefix)] = "string"
	case float64:
		out[shapeKey(prefix)] = "number"
	case bool:
		out[shapeKey(prefix)] = "bool"
	case nil:
		out[shapeKey(prefix)] = "null"
	default:
		out[shapeKey(prefix)] = "string"
	}
}

func shapeKey(prefix string) string {
	if prefix == "" {
		return "_"
	}
	return prefix
}

func writeFailedWire(t *testing.T, rec *wireRecorder, redactor *secretRedactor) {
	if rec == nil {
		return
	}
	root := strings.TrimSpace(os.Getenv(grokLiveFailedSessionsEnv))
	if root == "" {
		return
	}
	rejected := rec.non2xx()
	if len(rejected) == 0 {
		return
	}
	dir := filepath.Join(root, strings.ReplaceAll(t.Name(), "/", "_"), "wire")
	if err := writeWireDir(dir, rejected, redactor, os.Getenv(grokLiveWireBodiesEnv) == "1"); err != nil {
		t.Logf("preserve failed wire: %v", err)
	}
}

func writeWireDir(dir string, exchanges []wireExchange, redactor *secretRedactor, includeBodies bool) error {
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}
	for i, ex := range exchanges {
		nn := fmt.Sprintf("%02d", i+1)
		shapeJSON, err := json.MarshalIndent(shape(ex.requestBody), "", "  ")
		if err != nil {
			return err
		}
		shapeJSON = append(shapeJSON, '\n')
		if err := os.WriteFile(filepath.Join(dir, nn+"-request.shape.json"), shapeJSON, 0o600); err != nil {
			return err
		}
		body := string(ex.responseBody)
		if redactor != nil {
			body = redactor.redact(body)
		}
		text := fmt.Sprintf("status=%d\nmethod=%s\npath=%s\n\n%s", ex.status, ex.method, ex.path, body)
		if err := os.WriteFile(filepath.Join(dir, nn+"-response.txt"), []byte(text), 0o600); err != nil {
			return err
		}
		if !includeBodies {
			continue
		}
		reqBody := string(ex.requestBody)
		if redactor != nil {
			reqBody = redactor.redact(reqBody)
		}
		if err := os.WriteFile(filepath.Join(dir, nn+"-request.body.json"), []byte(reqBody), 0o600); err != nil {
			return err
		}
	}
	return nil
}

func TestShapeExtractsKeyPathTypes(t *testing.T) {
	body := []byte(`{
		"model": "grok-4.6",
		"stream": true,
		"n": 1,
		"temperature": 0.5,
		"empty_obj": {},
		"empty_arr": [],
		"maybe": null,
		"tools": [{"external_web_access": false, "name": "web"}],
		"input": [
			{"role": "user"},
			{"role": "assistant"},
			{"role": "user"},
			{"content": null, "type": "text"}
		]
	}`)
	want := map[string]string{
		"empty_arr":                    "array",
		"empty_obj":                    "object",
		"input[0].role":                "string",
		"input[1].role":                "string",
		"input[2].role":                "string",
		"input[3].content":             "null",
		"input[3].type":                "string",
		"maybe":                        "null",
		"model":                        "string",
		"n":                            "number",
		"stream":                       "bool",
		"temperature":                  "number",
		"tools[0].external_web_access": "bool",
		"tools[0].name":                "string",
	}
	got := shape(body)
	if !maps.Equal(got, want) {
		t.Fatalf("shape = %#v, want %#v", got, want)
	}
	if got := shape([]byte("not json")); !maps.Equal(got, map[string]string{"_": "non-json"}) {
		t.Fatalf("non-json shape = %#v", got)
	}
}

func TestWriteWireSelectsNon2xxAndNumbers(t *testing.T) {
	rec := newWireRecorder()
	rec.add(wireExchange{method: "POST", path: "/ok", status: 200, requestBody: []byte(`{"ok":true}`), responseBody: []byte("STREAM")})
	rec.add(wireExchange{
		method: "POST", path: "/v1/responses", status: 400,
		requestBody:  []byte(`{"tools":[{"external_web_access":true}]}`),
		responseBody: []byte("Argument not supported: external_web_access"),
	})
	rec.add(wireExchange{method: "GET", path: "/b", status: 500, requestBody: []byte(`{"n":1}`), responseBody: []byte("nope")})
	dir := filepath.Join(t.TempDir(), "wire")
	if err := writeWireDir(dir, rec.non2xx(), nil, false); err != nil {
		t.Fatal(err)
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	var names []string
	for _, entry := range entries {
		names = append(names, entry.Name())
	}
	wantNames := []string{"01-request.shape.json", "01-response.txt", "02-request.shape.json", "02-response.txt"}
	if strings.Join(names, ",") != strings.Join(wantNames, ",") {
		t.Fatalf("wire files = %v, want %v", names, wantNames)
	}
	gotShape, err := os.ReadFile(filepath.Join(dir, "01-request.shape.json"))
	if err != nil {
		t.Fatal(err)
	}
	wantShape, err := json.MarshalIndent(map[string]string{"tools[0].external_web_access": "bool"}, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	wantShape = append(wantShape, '\n')
	if string(gotShape) != string(wantShape) {
		t.Fatalf("01 shape = %s, want %s", gotShape, wantShape)
	}
	gotResp, err := os.ReadFile(filepath.Join(dir, "01-response.txt"))
	if err != nil {
		t.Fatal(err)
	}
	wantResp := "status=400\nmethod=POST\npath=/v1/responses\n\nArgument not supported: external_web_access"
	if string(gotResp) != wantResp {
		t.Fatalf("01 response = %q, want %q", gotResp, wantResp)
	}
	gotShape2, err := os.ReadFile(filepath.Join(dir, "02-request.shape.json"))
	if err != nil {
		t.Fatal(err)
	}
	wantShape2, err := json.MarshalIndent(map[string]string{"n": "number"}, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	wantShape2 = append(wantShape2, '\n')
	if string(gotShape2) != string(wantShape2) {
		t.Fatalf("02 shape = %s, want %s", gotShape2, wantShape2)
	}
}

func TestWireRecorderRingKeepsLastEight(t *testing.T) {
	rec := newWireRecorder()
	for i := 1; i <= 9; i++ {
		rec.add(wireExchange{status: 200 + i, path: fmt.Sprintf("/%d", i)})
	}
	got := rec.snapshot()
	want := make([]wireExchange, 8)
	for i := 0; i < 8; i++ {
		want[i] = wireExchange{status: 202 + i, path: fmt.Sprintf("/%d", i+2)}
	}
	if fmt.Sprintf("%v", got) != fmt.Sprintf("%v", want) {
		t.Fatalf("ring = %#v, want %#v", got, want)
	}
}

func TestWireRecorderRequestBodyCapMarksTruncation(t *testing.T) {
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNoContent)
	}))
	t.Cleanup(upstream.Close)
	server, rec, _ := startWireProxy(t, upstream.URL)
	t.Cleanup(server.Close)
	rec.requestBodyCap = 8
	resp, err := http.Post(server.URL+"/v1/responses", "application/json", bytes.NewReader(bytes.Repeat([]byte("x"), 20)))
	if err != nil {
		t.Fatal(err)
	}
	_, _ = io.Copy(io.Discard, resp.Body)
	_ = resp.Body.Close()
	got := rec.snapshot()
	if len(got) != 1 {
		t.Fatalf("exchanges = %d, want 1", len(got))
	}
	if !got[0].requestTruncated || len(got[0].requestBody) != 8 {
		t.Fatalf("truncated=%t len=%d body=%q", got[0].requestTruncated, len(got[0].requestBody), got[0].requestBody)
	}
	if got[0].status != http.StatusNoContent {
		t.Fatalf("status = %d", got[0].status)
	}
	if len(got[0].responseBody) != 0 {
		t.Fatalf("2xx response body was stored: %q", got[0].responseBody)
	}
}

func TestWireRecorderRoundTripWritesRejectedShapeAndResponse(t *testing.T) {
	secret := "WIRE-SECRET-123456"
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1/responses" {
			t.Errorf("upstream path = %s", r.URL.Path)
		}
		w.WriteHeader(http.StatusBadRequest)
		_, _ = w.Write([]byte(`{"error":"Argument not supported: external_web_access","api_key":"` + secret + `"}`))
	}))
	t.Cleanup(upstream.Close)
	server, rec, proxyBase := startWireProxy(t, upstream.URL+"/v1")
	t.Cleanup(server.Close)
	reqBody := []byte(`{"model":"grok-4.6","prompt":"` + secret + `","tools":[{"type":"function","external_web_access":true}],"input":[null,null,null,{"content":null}]}`)
	resp, err := http.Post(proxyBase+"/responses", "application/json", bytes.NewReader(reqBody))
	if err != nil {
		t.Fatal(err)
	}
	_, _ = io.Copy(io.Discard, resp.Body)
	_ = resp.Body.Close()
	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d", resp.StatusCode)
	}

	dir := filepath.Join(t.TempDir(), "wire")
	redactor := newSecretRedactor([]byte("api_key = \"" + secret + "\"\n"))
	if err := writeWireDir(dir, rec.non2xx(), redactor, true); err != nil {
		t.Fatal(err)
	}
	gotShape, err := os.ReadFile(filepath.Join(dir, "01-request.shape.json"))
	if err != nil {
		t.Fatal(err)
	}
	wantShape, err := json.MarshalIndent(map[string]string{
		"input[0]":                     "null",
		"input[1]":                     "null",
		"input[2]":                     "null",
		"input[3].content":             "null",
		"model":                        "string",
		"prompt":                       "string",
		"tools[0].external_web_access": "bool",
		"tools[0].type":                "string",
	}, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	wantShape = append(wantShape, '\n')
	if string(gotShape) != string(wantShape) {
		t.Fatalf("shape file = %s, want %s", gotShape, wantShape)
	}
	if strings.Contains(string(gotShape), secret) || strings.Contains(string(gotShape), "grok-4.6") {
		t.Fatalf("shape file contained a string value: %s", gotShape)
	}
	gotResp, err := os.ReadFile(filepath.Join(dir, "01-response.txt"))
	if err != nil {
		t.Fatal(err)
	}
	wantResp := "status=400\nmethod=POST\npath=/v1/responses\n\n" +
		`{"error":"Argument not supported: external_web_access","api_key":"[REDACTED]"}`
	if string(gotResp) != wantResp {
		t.Fatalf("response file = %q, want %q", gotResp, wantResp)
	}
	gotBody, err := os.ReadFile(filepath.Join(dir, "01-request.body.json"))
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(gotBody), secret) {
		t.Fatalf("request body retained secret: %s", gotBody)
	}
	if !strings.Contains(string(gotBody), "[REDACTED]") {
		t.Fatalf("request body missing redaction: %s", gotBody)
	}

	plainDir := filepath.Join(t.TempDir(), "wire-no-body")
	if err := writeWireDir(plainDir, rec.non2xx(), redactor, false); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(plainDir, "01-request.body.json")); err == nil {
		t.Fatal("request body file must not be written unless GROK_LIVE_WIRE_BODIES=1")
	}
}
