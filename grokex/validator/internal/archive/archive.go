// Package archive extracts one immutable Grokex release archive and records
// its identity.
package archive

import (
	"archive/tar"
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

// Release is an extracted archive.
type Release struct {
	// Name is the archive file name, e.g. grokex-v0.153.0-x86_64-unknown-linux-musl.tar.gz.
	Name string
	// SHA256 is the hex digest of the archive bytes.
	SHA256 string
	// Root is the single top-level directory inside the archive.
	Root string
}

// Binary is the app-server-capable Grokex binary inside the release.
func (r Release) Binary() string {
	return filepath.Join(r.Root, "bin", "grokex-bin")
}

// Version is the Grokex version the archive ships, read from its root
// directory (`grokex-v<version>`), so the validator reports the version it is
// actually driving instead of one it was compiled against.
func (r Release) Version() string {
	return strings.TrimPrefix(filepath.Base(r.Root), "grokex-v")
}

// SHA256File returns the hex SHA-256 of the file at path.
func SHA256File(path string) (string, error) {
	file, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer file.Close()
	digest := sha256.New()
	if _, err := io.Copy(digest, file); err != nil {
		return "", err
	}
	return hex.EncodeToString(digest.Sum(nil)), nil
}

// Extract unpacks the gzip tarball at path under destination and returns the
// release. Absolute members, parent traversal, and links are rejected because
// the archive is a downloaded artifact.
func Extract(path, destination string) (Release, error) {
	digest, err := SHA256File(path)
	if err != nil {
		return Release{}, fmt.Errorf("digest archive: %w", err)
	}
	file, err := os.Open(path)
	if err != nil {
		return Release{}, err
	}
	defer file.Close()
	gz, err := gzip.NewReader(file)
	if err != nil {
		return Release{}, fmt.Errorf("open archive: %w", err)
	}
	defer gz.Close()
	reader := tar.NewReader(gz)
	roots := map[string]struct{}{}
	for {
		header, err := reader.Next()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return Release{}, fmt.Errorf("read archive: %w", err)
		}
		name := filepath.Clean(header.Name)
		if name == "." || name == "" {
			continue
		}
		if filepath.IsAbs(header.Name) || strings.HasPrefix(name, "..") || strings.Contains(name, string(filepath.Separator)+"..") {
			return Release{}, fmt.Errorf("release archive contains an unsafe member: %s", header.Name)
		}
		roots[strings.SplitN(name, string(filepath.Separator), 2)[0]] = struct{}{}
		target := filepath.Join(destination, name)
		switch header.Typeflag {
		case tar.TypeDir:
			if err := os.MkdirAll(target, 0o755); err != nil {
				return Release{}, err
			}
		case tar.TypeReg:
			if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
				return Release{}, err
			}
			mode := os.FileMode(header.Mode) & 0o777
			if mode == 0 {
				mode = 0o644
			}
			out, err := os.OpenFile(target, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, mode)
			if err != nil {
				return Release{}, err
			}
			if _, err := io.Copy(out, reader); err != nil {
				out.Close()
				return Release{}, fmt.Errorf("extract %s: %w", header.Name, err)
			}
			if err := out.Close(); err != nil {
				return Release{}, err
			}
		default:
			return Release{}, fmt.Errorf("release archive contains an unsafe member: %s", header.Name)
		}
	}
	if len(roots) != 1 {
		return Release{}, errors.New("release archive does not have a single root")
	}
	var root string
	for root = range roots {
	}
	if !strings.HasPrefix(root, "grokex-v") {
		return Release{}, errors.New("release archive root is missing")
	}
	release := Release{Name: filepath.Base(path), SHA256: digest, Root: filepath.Join(destination, root)}
	if info, err := os.Stat(release.Binary()); err != nil || info.IsDir() {
		return Release{}, errors.New("release archive has no bin/grokex-bin")
	}
	return release, nil
}
