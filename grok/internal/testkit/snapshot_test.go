package testkit

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

func snapshotTestDigest(data []byte) string {
	digest := sha256.Sum256(data)
	return hex.EncodeToString(digest[:])
}

func snapshotTestWrite(t *testing.T, path string, data []byte, mode os.FileMode) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, data, mode); err != nil {
		t.Fatal(err)
	}
}

func snapshotTestJSON(t *testing.T, path string, value any) {
	t.Helper()
	data, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	snapshotTestWrite(t, path, append(data, '\n'), 0600)
}

func snapshotTestPackage(t *testing.T) (string, map[string]any) {
	t.Helper()
	root := t.TempDir()
	files := map[string]string{
		"bin/grok":                 "#!/bin/sh\nexec \"$(dirname \"$0\")/grok-bin\"\n",
		"bin/grok-bin":             "#!/bin/sh\nd=$(dirname \"$0\")\nprintf 'runtime-A/'\n\"$d/codex-code-mode-host\"\n\"$d/../codex-path/rg\"\n\"$d/../codex-resources/bwrap\"\ncat \"$d/../codex-resources/data\"\n",
		"bin/codex-code-mode-host": "#!/bin/sh\nprintf 'helper-A/'\n",
		"codex-path/rg":            "#!/bin/sh\nprintf 'path-A/'\n",
		"codex-resources/bwrap":    "#!/bin/sh\nprintf 'resource-A/'\n",
		"codex-resources/data":     "data-A\n",
		"LICENSE":                  "fixture license\n",
		"codex-package.json":       `{"layoutVersion":1,"version":"0.1.0","target":"x86_64-unknown-linux-musl","variant":"grok","entrypoint":"bin/grok-bin","resourcesDir":"codex-resources","pathDir":"codex-path"}`,
	}
	identities := map[string]any{}
	for name, content := range files {
		mode := os.FileMode(0600)
		if strings.HasPrefix(content, "#!/bin/sh") {
			mode = 0700
		}
		snapshotTestWrite(t, filepath.Join(root, filepath.FromSlash(name)), []byte(content), mode)
		identities[name] = map[string]any{"size": len(content), "sha256": snapshotTestDigest([]byte(content))}
	}
	manifest := map[string]any{
		"schema_version": 1, "entrypoint": "bin/grok", "runtime": "bin/grok-bin",
		"target": "x86_64-unknown-linux-musl", "version": "0.1.0", "files": identities,
		"repository": "fixture/unverified", "source_sha": "unverified-fixture",
	}
	snapshotTestJSON(t, filepath.Join(root, "grok-package.json"), manifest)
	return root, manifest
}

func snapshotTestRun(t *testing.T, binary, want string) {
	t.Helper()
	got, err := exec.Command(binary).CombinedOutput()
	if err != nil || string(got) != want {
		t.Fatalf("snapshot execution: got %q, error %v; want %q", got, err, want)
	}
}

func snapshotTestRejected(t *testing.T, path, digest string) {
	t.Helper()
	binary, cleanup, err := SnapshotExecutable(path, digest)
	if cleanup != nil {
		cleanup()
	}
	if err == nil || binary != "" || cleanup != nil {
		t.Fatal("invalid snapshot accepted or partial snapshot returned")
	}
	if strings.Contains(err.Error(), filepath.Dir(path)) || strings.Contains(err.Error(), "PRIVATE") || strings.Contains(err.Error(), "fixture/unverified") {
		t.Fatal("snapshot error leaked source path or metadata")
	}
}

func TestSnapshotExecutableReplacementAndCleanup(t *testing.T) {
	root := t.TempDir()
	source := filepath.Join(root, "codex")
	helper := "codex-code-mode-host"
	if runtime.GOOS == "windows" {
		helper += ".exe"
	}
	a := []byte("#!/bin/sh\nprintf 'A/'\n\"$(dirname \"$0\")/codex-code-mode-host\"\n")
	helperA := []byte("#!/bin/sh\nprintf 'helper-A\\n'\n")
	snapshotTestWrite(t, source, a, 0700)
	snapshotTestWrite(t, filepath.Join(root, helper), helperA, 0700)
	snapshotTestWrite(t, filepath.Join(root, "unrelated-build-output"), []byte("do not copy"), 0600)
	binary, cleanup, err := SnapshotExecutable(source, snapshotTestDigest(a))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(cleanup)
	if binary == source || filepath.Base(binary) != "codex" {
		t.Fatal("snapshot did not retain an independent executable basename")
	}
	for _, name := range []string{"codex", helper} {
		info, err := os.Stat(filepath.Join(filepath.Dir(binary), name))
		if err != nil || (runtime.GOOS != "windows" && info.Mode().Perm() != 0500) {
			t.Fatal("snapshot executable must be owner-readable/executable and non-writable")
		}
	}
	if info, err := os.Stat(filepath.Dir(binary)); err != nil || (runtime.GOOS != "windows" && info.Mode().Perm() != 0700) {
		t.Fatal("snapshot directory must be private")
	}
	if _, err := os.Stat(filepath.Join(filepath.Dir(binary), "unrelated-build-output")); !os.IsNotExist(err) {
		t.Fatal("raw snapshot copied unrelated build output")
	}
	// Reproduce the pre-hash / replace-B / invoke / restore-A / post-hash gap.
	snapshotTestWrite(t, source, []byte("#!/bin/sh\nprintf 'B\\n'\n"), 0700)
	snapshotTestWrite(t, filepath.Join(root, helper), []byte("#!/bin/sh\nprintf 'helper-B\\n'\n"), 0700)
	if runtime.GOOS != "windows" {
		snapshotTestRun(t, source, "B\n")
		snapshotTestRun(t, binary, "A/helper-A\n")
	}
	for name, want := range map[string][]byte{"codex": a, helper: helperA} {
		got, err := os.ReadFile(filepath.Join(filepath.Dir(binary), name))
		if err != nil || string(got) != string(want) {
			t.Fatal("source replacement changed snapshotted bytes")
		}
	}
	snapshotTestWrite(t, source, a, 0700)
	if digest, err := ExecutableSHA256(source); err != nil || digest != snapshotTestDigest(a) {
		t.Fatal("replacement control failed to restore the original digest")
	}
	cleanup()
	cleanup()
	if _, err := os.Stat(filepath.Dir(binary)); !os.IsNotExist(err) {
		t.Fatal("snapshot cleanup did not remove its owned directory")
	}
	if data, err := os.ReadFile(source); err != nil || string(data) != string(a) {
		t.Fatal("snapshot cleanup changed source files")
	}
}

func TestSnapshotExecutablePackageLayout(t *testing.T) {
	for _, selected := range []string{"grok", "grok-bin"} {
		t.Run(selected, func(t *testing.T) {
			root, manifest := snapshotTestPackage(t)
			source := filepath.Join(root, "bin", selected)
			data, err := os.ReadFile(source)
			if err != nil {
				t.Fatal(err)
			}
			binary, cleanup, err := SnapshotExecutable(source, snapshotTestDigest(data))
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(cleanup)
			snapshotRoot := filepath.Dir(filepath.Dir(binary))
			names := []string{"grok-package.json"}
			for name := range manifest["files"].(map[string]any) {
				names = append(names, name)
			}
			for _, name := range names {
				before, readErr := os.ReadFile(filepath.Join(root, filepath.FromSlash(name)))
				after, snapshotErr := os.ReadFile(filepath.Join(snapshotRoot, filepath.FromSlash(name)))
				if readErr != nil || snapshotErr != nil || string(before) != string(after) {
					t.Fatalf("package file not preserved: %s", name)
				}
				if runtime.GOOS != "windows" {
					info, err := os.Stat(filepath.Join(snapshotRoot, filepath.FromSlash(name)))
					if err != nil || info.Mode().Perm()&0222 != 0 {
						t.Fatal("snapshot file remained writable")
					}
				}
			}
			// Removing the entire source package also exercises helper/resource
			// lookup from the copied executable, rather than from its old location.
			if err := os.RemoveAll(root); err != nil {
				t.Fatal(err)
			}
			if runtime.GOOS != "windows" {
				snapshotTestRun(t, binary, "runtime-A/helper-A/path-A/resource-A/data-A\n")
			}
		})
	}
}

func TestSnapshotExecutableRejectsInvalidInputs(t *testing.T) {
	root := t.TempDir()
	source := filepath.Join(root, "PRIVATE-codex")
	data := []byte("synthetic executable")
	snapshotTestWrite(t, source, data, 0700)
	digest := snapshotTestDigest(data)
	for _, invalid := range []string{"", "PRIVATE", digest[:63], strings.ToUpper(digest), strings.Repeat("0", 64)} {
		snapshotTestRejected(t, source, invalid)
	}
	for _, path := range []string{"relative", source + "-missing", root} {
		snapshotTestRejected(t, path, digest)
	}
	if runtime.GOOS != "windows" {
		if err := os.Chmod(source, 0600); err != nil {
			t.Fatal(err)
		}
		snapshotTestRejected(t, source, digest)
		if err := os.Chmod(source, 0700); err != nil {
			t.Fatal(err)
		}
	}
	for _, size := range []int64{0, snapshotFileLimit + 1} {
		if err := os.Truncate(source, size); err != nil {
			t.Fatal(err)
		}
		snapshotTestRejected(t, source, digest)
	}
}

func TestSnapshotExecutableRawWithoutHelper(t *testing.T) {
	root := t.TempDir()
	source := filepath.Join(root, "codex.exe")
	data := []byte("raw executable fixture")
	snapshotTestWrite(t, source, data, 0700)
	binary, cleanup, err := SnapshotExecutable(source, snapshotTestDigest(data))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(cleanup)
	got, readErr := os.ReadFile(binary)
	if readErr != nil || string(got) != string(data) || filepath.Base(binary) != "codex.exe" {
		t.Fatal("raw executable bytes or basename changed")
	}
	cleanup()
	if _, err := os.Stat(filepath.Dir(binary)); !os.IsNotExist(err) {
		t.Fatal("cleanup failed to remove read-only snapshot")
	}
}

func TestSnapshotExecutableFailureCleanup(t *testing.T) {
	// Give the child its own temporary-directory environment rather than
	// changing the test process's global environment or scanning shared /tmp.
	if source := os.Getenv("GROK_SNAPSHOT_CLEANUP_FIXTURE"); source != "" {
		snapshotTestRejected(t, source, strings.Repeat("0", 64))
		return
	}
	root, temporary := t.TempDir(), t.TempDir()
	source := filepath.Join(root, "codex")
	snapshotTestWrite(t, source, []byte("executable fixture"), 0700)
	cmd := exec.Command(os.Args[0], "-test.run=^TestSnapshotExecutableFailureCleanup$")
	cmd.Env = append(os.Environ(), "GROK_SNAPSHOT_CLEANUP_FIXTURE="+source, "TMPDIR="+temporary, "TMP="+temporary, "TEMP="+temporary)
	if output, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("failure-cleanup fixture: %v, %s", err, output)
	}
	entries, err := os.ReadDir(temporary)
	if err != nil || len(entries) != 0 {
		t.Fatal("failed snapshot left temporary files behind")
	}
}

func TestSnapshotExecutableRejectsPackageMutations(t *testing.T) {
	for _, change := range []string{
		"missing-manifest", "missing-layout", "missing-helper", "malformed-manifest", "oversized-manifest", "duplicate-key", "duplicate-path", "trailing-json", "deep-json", "wrong-layout",
		"traversal", "absolute", "backslash", "unclean", "case-collision", "self-manifest", "missing-size", "negative-size", "oversized-file", "oversized-total", "too-many-files", "invalid-digest", "wrong-selected-digest", "tamper-selected", "tamper-helper", "size-mismatch",
	} {
		t.Run(change, func(t *testing.T) {
			root, manifest := snapshotTestPackage(t)
			files := manifest["files"].(map[string]any)
			source := filepath.Join(root, "bin", "grok")
			selected := files["bin/grok"].(map[string]any)
			digest := selected["sha256"].(string)
			manifestPath := filepath.Join(root, "grok-package.json")
			switch change {
			case "missing-manifest", "missing-layout", "missing-helper":
				name := map[string]string{"missing-manifest": "grok-package.json", "missing-layout": "codex-package.json", "missing-helper": "bin/codex-code-mode-host"}[change]
				if err := os.Remove(filepath.Join(root, filepath.FromSlash(name))); err != nil {
					t.Fatal(err)
				}
			case "malformed-manifest":
				snapshotTestWrite(t, manifestPath, []byte("PRIVATE invalid JSON"), 0600)
			case "oversized-manifest":
				if err := os.Truncate(manifestPath, snapshotMetadataLimit+1); err != nil {
					t.Fatal(err)
				}
			case "duplicate-key", "trailing-json", "deep-json", "duplicate-path":
				data, err := os.ReadFile(manifestPath)
				if err != nil {
					t.Fatal(err)
				}
				if change == "duplicate-key" {
					data = append([]byte(`{"schema_version":0,`), data[1:]...)
				}
				if change == "trailing-json" {
					data = append(data, []byte(" {}")...)
				}
				if change == "deep-json" {
					data = []byte(strings.Repeat("[", 18) + "0" + strings.Repeat("]", 18))
				}
				if change == "duplicate-path" {
					data = []byte(strings.Replace(string(data), `"files": {`, `"files": {"bin/grok": {},`, 1))
				}
				snapshotTestWrite(t, manifestPath, data, 0600)
			case "wrong-layout":
				data := []byte(`{"layoutVersion":2}`)
				snapshotTestWrite(t, filepath.Join(root, "codex-package.json"), data, 0600)
				files["codex-package.json"] = map[string]any{"size": len(data), "sha256": snapshotTestDigest(data)}
				snapshotTestJSON(t, manifestPath, manifest)
			case "tamper-selected", "tamper-helper", "size-mismatch":
				name := "bin/grok"
				if change == "tamper-helper" {
					name = "bin/codex-code-mode-host"
				}
				data, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(name)))
				if err != nil {
					t.Fatal(err)
				}
				data[len(data)-1] = '!'
				if change == "size-mismatch" {
					data = append(data, '!')
				}
				snapshotTestWrite(t, filepath.Join(root, filepath.FromSlash(name)), data, 0700)
			default:
				switch change {
				case "traversal", "absolute", "backslash", "unclean", "case-collision", "self-manifest":
					name := map[string]string{"traversal": "../PRIVATE", "absolute": "/PRIVATE", "backslash": `bin\PRIVATE`, "unclean": "bin/./PRIVATE", "case-collision": "BIN/GROK", "self-manifest": "grok-package.json"}[change]
					files[name] = selected
				case "missing-size":
					delete(selected, "size")
				case "negative-size":
					selected["size"] = -1
				case "oversized-file":
					selected["size"] = snapshotFileLimit + 1
				case "oversized-total":
					selected["size"] = snapshotFileLimit
					files["bin/grok-bin"].(map[string]any)["size"] = snapshotFileLimit
				case "too-many-files":
					for i := range snapshotFileCount {
						files[fmt.Sprintf("extra/%d", i)] = selected
					}
				case "invalid-digest":
					selected["sha256"] = "PRIVATE"
				case "wrong-selected-digest":
					selected["sha256"] = strings.Repeat("0", 64)
				}
				snapshotTestJSON(t, manifestPath, manifest)
			}
			snapshotTestRejected(t, source, digest)
		})
	}
}
