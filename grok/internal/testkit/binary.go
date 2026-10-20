// Package testkit owns bounded executable selection for the Go test tiers.
package testkit

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"io"
	"os"
	"path/filepath"
	"runtime"
)

// SelectBinary resolves the explicit input, retaining the legacy spelling only
// when it does not disagree with GROK_CODEX_BIN. It never searches PATH.
func SelectBinary(getenv func(string) string) (string, error) {
	binary, legacy := getenv("GROK_CODEX_BIN"), getenv("GROK_LIVE_NATIVE_BIN")
	if binary == "" {
		binary = legacy
	}
	if binary == "" {
		return "", errors.New("testkit: GROK_CODEX_BIN is required")
	}
	if !filepath.IsAbs(binary) || (legacy != "" && !filepath.IsAbs(legacy)) {
		return "", errors.New("testkit: executable path must be absolute")
	}
	if legacy != "" && filepath.Clean(binary) != filepath.Clean(legacy) {
		return "", errors.New("testkit: GROK_CODEX_BIN conflicts with GROK_LIVE_NATIVE_BIN")
	}
	return filepath.Clean(binary), nil
}

// ExecutableSHA256 rejects non-executable, non-regular and oversized inputs.
// Its errors intentionally exclude paths and file contents.
func ExecutableSHA256(path string) (string, error) {
	if !filepath.IsAbs(path) {
		return "", errors.New("testkit: executable path must be absolute")
	}
	// Reject pipes/devices before opening: opening a FIFO can block forever.
	before, err := os.Stat(path)
	if err != nil {
		return "", errors.New("testkit: executable unavailable")
	}
	if !before.Mode().IsRegular() || before.Size() <= 0 || before.Size() > 1<<30 ||
		(runtime.GOOS != "windows" && before.Mode().Perm()&0111 == 0) {
		return "", errors.New("testkit: invalid executable")
	}
	file, err := os.Open(path)
	if err != nil {
		return "", errors.New("testkit: executable unavailable")
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil || !os.SameFile(before, info) || !info.Mode().IsRegular() || info.Size() != before.Size() ||
		(runtime.GOOS != "windows" && info.Mode().Perm()&0111 == 0) {
		return "", errors.New("testkit: executable changed before hashing")
	}
	hash := sha256.New()
	count, err := io.Copy(hash, io.LimitReader(file, (1<<30)+1))
	if err != nil || count != info.Size() {
		return "", errors.New("testkit: executable digest unavailable")
	}
	return hex.EncodeToString(hash.Sum(nil)), nil
}
