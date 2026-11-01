//go:build !windows

package testkit

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func TestSnapshotExecutableRejectsLinksAndSpecialFiles(t *testing.T) {
	for _, target := range []string{"selected", "helper", "exec-server", "manifest", "directory", "fifo", "raw-helper", "raw-exec-server", "raw-selected"} {
		t.Run(target, func(t *testing.T) {
			root, manifest := snapshotTestPackage(t)
			source := filepath.Join(root, "bin", "grok")
			digest := manifest["files"].(map[string]any)["bin/grok"].(map[string]any)["sha256"].(string)
			name := map[string]string{"selected": "bin/grok", "helper": "bin/codex-code-mode-host", "exec-server": "bin/codex-exec-server", "manifest": "grok-package.json", "directory": "codex-resources", "fifo": "bin/grok", "raw-helper": "bin/codex-code-mode-host", "raw-exec-server": "bin/codex-exec-server", "raw-selected": "bin/grok"}[target]
			if strings.HasPrefix(target, "raw-") {
				raw := t.TempDir()
				for _, base := range []string{"grok", "codex-code-mode-host", "codex-exec-server"} {
					data, err := os.ReadFile(filepath.Join(root, "bin", base))
					if err != nil {
						t.Fatal(err)
					}
					snapshotTestWrite(t, filepath.Join(raw, base), data, 0700)
				}
				root, source, name = raw, filepath.Join(raw, "grok"), filepath.Base(name)
			}
			path := filepath.Join(root, filepath.FromSlash(name))
			if err := os.Rename(path, path+"-original"); err != nil {
				t.Fatal(err)
			}
			if target == "fifo" {
				if err := exec.Command("mkfifo", path).Run(); err != nil {
					t.Fatal(err)
				}
			} else if err := os.Symlink(filepath.Base(path)+"-original", path); err != nil {
				t.Fatal(err)
			}
			snapshotTestRejected(t, source, digest)
		})
	}
}
