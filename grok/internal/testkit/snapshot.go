package testkit

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"sync"
)

const (
	snapshotFileLimit     = 1 << 30
	snapshotTotalLimit    = 2 << 30
	snapshotMetadataLimit = 1 << 20
	snapshotFileCount     = 128
)

var errSnapshot = errors.New("testkit: executable snapshot unavailable or invalid")

type snapshotIdentity struct {
	Size   *int64 `json:"size"`
	SHA256 string `json:"sha256"`
}

type snapshotManifest struct {
	SchemaVersion int                         `json:"schema_version"`
	Entrypoint    string                      `json:"entrypoint"`
	Runtime       string                      `json:"runtime"`
	Target        string                      `json:"target"`
	Version       string                      `json:"version"`
	Files         map[string]snapshotIdentity `json:"files"`
}

// SnapshotExecutable binds execution to copied, verified bytes, independently of
// later replacements of the source path. The caller must retain the snapshot
// until all child processes have joined, then call the idempotent cleanup.
//
// Canonical bin/grok and bin/grok-bin inputs require both package manifests; all
// manifest-listed files retain their relative layout. Other inputs retain their
// basename and optional adjacent codex-code-mode-host[.exe] and
// codex-exec-server[.exe]. No arbitrary build directory is copied. Symlinks within the source root are rejected; ancestor
// directory aliases are allowed, with source reads confined to an opened root.
// Files are owner-readable and non-writable, with owner execution preserved.
// This protects against ordinary builds replacing source files, not malicious
// same-user mutation of the private snapshot. It makes no provenance claim.
// A raw helper has no caller-supplied digest: its copied bytes are stable, but
// this does not establish a coherent executable/helper build identity.
func SnapshotExecutable(path, expectedSHA256 string) (binary string, cleanup func(), err error) {
	if !filepath.IsAbs(path) || !snapshotDigest(expectedSHA256) {
		return "", nil, errSnapshot
	}
	path = filepath.Clean(path)
	sourceDir, selected := filepath.Dir(path), filepath.Base(path)
	packaged := filepath.Base(sourceDir) == "bin" && (selected == "grok" || selected == "grok-bin")
	if filepath.Base(sourceDir) == "bin" {
		// A damaged/unsupported package must not silently become a raw binary.
		for _, marker := range []string{"grok-package.json", "codex-package.json", "codex-resources", "codex-path"} {
			_, statErr := os.Lstat(filepath.Join(filepath.Dir(sourceDir), marker))
			if !errors.Is(statErr, fs.ErrNotExist) {
				packaged = true
			}
		}
	}
	if packaged {
		sourceDir, selected = filepath.Dir(sourceDir), "bin/"+selected
	}
	source, openErr := os.OpenRoot(sourceDir)
	if openErr != nil {
		return "", nil, errSnapshot
	}
	defer source.Close()
	var manifest snapshotManifest
	var metadata []byte
	if packaged {
		metadata, err = snapshotReadMetadata(source, "grok-package.json")
		if err != nil || snapshotJSON(metadata, &manifest) != nil || validateSnapshotManifest(manifest, len(metadata), selected, expectedSHA256) != nil {
			return "", nil, errSnapshot
		}
		layout, readErr := snapshotReadMetadata(source, "codex-package.json")
		var fields struct {
			LayoutVersion int    `json:"layoutVersion"`
			Entrypoint    string `json:"entrypoint"`
			ExecServer    string `json:"execServer"`
			ResourcesDir  string `json:"resourcesDir"`
			PathDir       string `json:"pathDir"`
			Variant       string `json:"variant"`
			Target        string `json:"target"`
			Version       string `json:"version"`
		}
		identity := manifest.Files["codex-package.json"]
		digest := sha256.Sum256(layout)
		if readErr != nil || int64(len(layout)) != *identity.Size || hex.EncodeToString(digest[:]) != identity.SHA256 ||
			snapshotJSON(layout, &fields) != nil || fields.LayoutVersion != 1 || fields.Entrypoint != "bin/grok-bin" ||
			fields.ExecServer != "bin/codex-exec-server" || fields.ResourcesDir != "codex-resources" || fields.PathDir != "codex-path" || fields.Variant != "grok" ||
			fields.Target != manifest.Target || fields.Version != manifest.Version {
			return "", nil, errSnapshot
		}
	}
	dir, makeErr := os.MkdirTemp("", "grok-execution-")
	if makeErr != nil {
		return "", nil, errSnapshot
	}
	remove := sync.OnceFunc(func() { _ = os.RemoveAll(dir) })
	defer func() {
		if err != nil {
			remove()
		}
	}()
	if packaged {
		names := make([]string, 0, len(manifest.Files))
		for name := range manifest.Files {
			names = append(names, name)
		}
		sort.Strings(names)
		for _, name := range names {
			identity := manifest.Files[name]
			if err = snapshotCopy(source, dir, name, identity.SHA256, *identity.Size, snapshotRequiredExecutable(name)); err != nil {
				return "", nil, err
			}
		}
		if err = os.WriteFile(filepath.Join(dir, "grok-package.json"), metadata, 0400); err != nil {
			return "", nil, errSnapshot
		}
	} else {
		if err = snapshotCopy(source, dir, selected, expectedSHA256, -1, true); err != nil {
			return "", nil, err
		}
		for _, helper := range []string{"codex-code-mode-host", "codex-exec-server"} {
			if runtime.GOOS == "windows" {
				helper += ".exe"
			}
			if helper != selected {
				_, statErr := source.Lstat(helper)
				if !errors.Is(statErr, fs.ErrNotExist) {
					if err = snapshotCopy(source, dir, helper, "", -1, true); err != nil {
						return "", nil, err
					}
				}
			}
		}
	}
	return filepath.Join(dir, filepath.FromSlash(selected)), remove, nil
}

func snapshotDigest(value string) bool {
	if len(value) != sha256.Size*2 {
		return false
	}
	decoded, err := hex.DecodeString(value)
	return err == nil && len(decoded) == sha256.Size && value == strings.ToLower(value)
}

func validateSnapshotManifest(m snapshotManifest, metadataSize int, selected, expected string) error {
	if m.SchemaVersion != 1 || m.Entrypoint != "bin/grok" || m.Runtime != "bin/grok-bin" || m.Target == "" || m.Version == "" ||
		len(m.Files) == 0 || len(m.Files)+1 > snapshotFileCount || (selected != m.Entrypoint && selected != m.Runtime) {
		return errSnapshot
	}
	total := int64(metadataSize)
	seen := make(map[string]bool, len(m.Files))
	for name, identity := range m.Files {
		if !fs.ValidPath(name) || name == "." || strings.EqualFold(name, "grok-package.json") || len(name) > 512 || strings.Count(name, "/") > 16 ||
			strings.ContainsAny(name, "\\:") || seen[strings.ToLower(name)] || identity.Size == nil || *identity.Size < 0 ||
			*identity.Size > snapshotFileLimit || !snapshotDigest(identity.SHA256) {
			return errSnapshot
		}
		if _, err := filepath.Localize(name); err != nil {
			return errSnapshot
		}
		seen[strings.ToLower(name)] = true
		total += *identity.Size
		if total > snapshotTotalLimit {
			return errSnapshot
		}
	}
	required := []string{"bin/grok", "bin/grok-bin", "bin/codex-code-mode-host", "bin/codex-exec-server", "codex-package.json", "codex-path/rg"}
	if strings.Contains(m.Target, "linux") {
		required = append(required, "codex-resources/bwrap")
	}
	for _, name := range required {
		if identity, ok := m.Files[name]; !ok || *identity.Size <= 0 {
			return errSnapshot
		}
	}
	if m.Files[selected].SHA256 != expected || *m.Files["codex-package.json"].Size > snapshotMetadataLimit {
		return errSnapshot
	}
	return nil
}

func snapshotRequiredExecutable(name string) bool {
	switch name {
	case "bin/grok", "bin/grok-bin", "bin/codex-code-mode-host", "bin/codex-exec-server", "codex-path/rg", "codex-resources/bwrap":
		return true
	}
	return false
}

func snapshotOpen(source *os.Root, name string, limit int64, executable bool) (*os.File, os.FileInfo, error) {
	parts := strings.Split(filepath.ToSlash(name), "/")
	for i := range parts {
		info, err := source.Lstat(filepath.Join(parts[:i+1]...))
		if err != nil || info.Mode()&os.ModeSymlink != 0 || (i < len(parts)-1 && !info.IsDir()) {
			return nil, nil, errSnapshot
		}
	}
	before, err := source.Lstat(name)
	if err != nil || !before.Mode().IsRegular() || before.Size() > limit ||
		(executable && (before.Size() <= 0 || (runtime.GOOS != "windows" && before.Mode().Perm()&0111 == 0))) {
		return nil, nil, errSnapshot
	}
	file, err := source.Open(name)
	if err != nil {
		return nil, nil, errSnapshot
	}
	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() || !os.SameFile(before, info) || before.Size() != info.Size() || before.Mode() != info.Mode() {
		file.Close()
		return nil, nil, errSnapshot
	}
	return file, info, nil
}

func snapshotReadMetadata(source *os.Root, name string) ([]byte, error) {
	file, info, err := snapshotOpen(source, name, snapshotMetadataLimit, false)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, snapshotMetadataLimit+1))
	if err != nil || int64(len(data)) != info.Size() {
		return nil, errSnapshot
	}
	return data, nil
}

func snapshotCopy(source *os.Root, destination, name, expected string, size int64, executable bool) error {
	file, info, err := snapshotOpen(source, name, snapshotFileLimit, executable)
	if err != nil {
		return err
	}
	defer file.Close()
	if size >= 0 && info.Size() != size {
		return errSnapshot
	}
	target := filepath.Join(destination, filepath.FromSlash(name))
	if err := os.MkdirAll(filepath.Dir(target), 0700); err != nil {
		return errSnapshot
	}
	output, err := os.OpenFile(target, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return errSnapshot
	}
	defer output.Close()
	hash := sha256.New()
	count, err := io.Copy(io.MultiWriter(output, hash), io.LimitReader(file, info.Size()+1))
	if err != nil || count != info.Size() || (expected != "" && hex.EncodeToString(hash.Sum(nil)) != expected) {
		return errSnapshot
	}
	mode := os.FileMode(0400)
	if info.Mode().Perm()&0111 != 0 || executable {
		mode |= 0100
	}
	if output.Chmod(mode) != nil || output.Close() != nil {
		return errSnapshot
	}
	return nil
}

// Reject ambiguous duplicate keys and excessively deep metadata before typed
// decoding. The byte limit is enforced by snapshotReadMetadata.
func snapshotJSON(data []byte, target any) error {
	decoder := json.NewDecoder(bytes.NewReader(data))
	var value func(int) error
	value = func(depth int) error {
		if depth > 16 {
			return errSnapshot
		}
		token, err := decoder.Token()
		if err != nil {
			return errSnapshot
		}
		if delim, ok := token.(json.Delim); ok {
			keys := map[string]bool{}
			for decoder.More() {
				if delim == '{' {
					key, err := decoder.Token()
					name, ok := key.(string)
					if err != nil || !ok || keys[name] {
						return errSnapshot
					}
					keys[name] = true
				}
				if err := value(depth + 1); err != nil {
					return err
				}
			}
			if _, err := decoder.Token(); err != nil {
				return errSnapshot
			}
		}
		return nil
	}
	if value(0) != nil {
		return errSnapshot
	}
	if _, err := decoder.Token(); err != io.EOF {
		return errSnapshot
	}
	if json.Unmarshal(data, target) != nil {
		return errSnapshot
	}
	return nil
}
