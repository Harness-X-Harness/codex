package testkit

import (
	"crypto/sha256"
	"encoding/hex"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

func TestSelectBinary(t *testing.T) {
	binary := filepath.Join(t.TempDir(), "codex")
	for _, tc := range []struct {
		name, preferred, legacy string
		valid                   bool
	}{
		{"preferred", binary, "", true},
		{"legacy", "", binary, true},
		{"same", binary, binary, true},
		{"conflict", binary, binary + "-different", false},
		{"missing", "", "", false},
		{"relative", "codex", "", false},
		{"relative_alias", binary, "codex", false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			values := map[string]string{"GROK_CODEX_BIN": tc.preferred, "GROK_LIVE_NATIVE_BIN": tc.legacy}
			got, err := SelectBinary(func(key string) string { return values[key] })
			if (err == nil) != tc.valid || (tc.valid && got != binary) {
				t.Fatalf("selection mismatch: valid=%t error=%v", tc.valid, err)
			}
		})
	}
}

func TestExecutableSHA256(t *testing.T) {
	binary := filepath.Join(t.TempDir(), "codex")
	data := []byte("executable fixture")
	if err := os.WriteFile(binary, data, 0700); err != nil {
		t.Fatal(err)
	}
	got, err := ExecutableSHA256(binary)
	want := sha256.Sum256(data)
	if err != nil || got != hex.EncodeToString(want[:]) {
		t.Fatal("wrong executable digest")
	}
	for _, path := range []string{"relative", binary + ".PRIVATE_PATH", filepath.Dir(binary)} {
		if _, err := ExecutableSHA256(path); err == nil || strings.Contains(err.Error(), "PRIVATE_PATH") {
			t.Fatal("invalid path accepted or leaked")
		}
	}
	if runtime.GOOS != "windows" {
		if err := os.Chmod(binary, 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := ExecutableSHA256(binary); err == nil {
			t.Fatal("non-executable file accepted")
		}
		if err := os.Chmod(binary, 0700); err != nil {
			t.Fatal(err)
		}
	}
	for _, size := range []int64{0, (1 << 30) + 1} {
		if err := os.Truncate(binary, size); err != nil {
			t.Fatal(err)
		}
		if _, err := ExecutableSHA256(binary); err == nil {
			t.Fatal("invalid executable size accepted")
		}
	}
}
