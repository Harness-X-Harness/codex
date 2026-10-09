"""Exercise the real staging command with isolated, synthetic executable inputs."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
LINUX = "x86_64-unknown-linux-musl"
MAC = "aarch64-apple-darwin"


class StageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo"
        for directory in ("grok/dist", "grok/package", "scripts/codex_package"):
            shutil.copytree(ROOT / directory, self.repo / directory, ignore=shutil.ignore_patterns("__pycache__"))
        for name in ("LICENSE", "codex-rs/Cargo.toml"):
            destination = self.repo / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / name, destination)
        for command in (
            ["init", "--quiet"], ["add", "."],
            ["-c", "user.name=Package fixture", "-c", "user.email=fixture@example.invalid", "commit", "--quiet", "-m", "fixture"],
        ):
            subprocess.run(["git", *command], cwd=self.repo, check=True, capture_output=True)
        self.sha = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=self.repo, text=True).strip()
        self.inputs = self.root / "inputs"
        self.inputs.mkdir()
        for name in ("runtime", "code-mode-host", "rg", "zsh", "bwrap"):
            path = self.inputs / name
            path.write_text("#!/usr/bin/env python3\nimport json,os,sys\nprint(json.dumps([os.environ.get('CODEX_HOME'),sys.argv[1:]]))\n# fixture " + name + "\n")
            path.chmod(0o755)
        self.output = self.root / "package"

    def invoke(self, target=LINUX, extra=(), success=True):
        command = [sys.executable, str(self.repo / "grok/package/stage.py"),
                   "--target", target, "--source-sha", self.sha, "--output", str(self.output)]
        for name in ("runtime", "code-mode-host", "rg", "zsh"):
            command += ["--" + name, str(self.inputs / name)]
        if target == LINUX:
            command += ["--bwrap", str(self.inputs / "bwrap")]
        result = subprocess.run(command + list(extra), capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode == 0, success, result.stderr)
        return result

    def test_complete_linux_layout_and_exact_identity(self):
        self.invoke()
        files = {p.relative_to(self.output).as_posix() for p in self.output.rglob("*") if p.is_file()}
        expected = {"bin/grok", "bin/grok-bin", "bin/codex-code-mode-host", "codex-path/rg",
                    "codex-resources/zsh/bin/zsh", "codex-resources/bwrap", "codex-package.json",
                    "grok-package.json", "config.toml.example", "models.json", "INSTALL.md", "LICENSE"}
        self.assertEqual(files, expected)
        metadata = json.loads((self.output / "grok-package.json").read_text())
        identities = {name: {"size": len((self.output / name).read_bytes()),
                             "sha256": hashlib.sha256((self.output / name).read_bytes()).hexdigest()}
                      for name in files - {"grok-package.json"}}
        self.assertEqual(metadata, {"schema_version": 1, "repository": "Harness-X-Harness/codex",
                                   "source_sha": self.sha, "target": LINUX, "version": "0.158.0",
                                   "entrypoint": "bin/grok", "runtime": "bin/grok-bin", "files": identities})
        self.assertEqual(json.loads((self.output / "codex-package.json").read_text()),
                         {"layoutVersion": 1, "version": "0.158.0", "target": LINUX, "variant": "grok",
                          "entrypoint": "bin/grok-bin", "resourcesDir": "codex-resources", "pathDir": "codex-path"})
        for asset in ("config.toml.example", "models.json"):
            self.assertEqual((self.output / asset).read_bytes(), (ROOT / "grok/dist" / asset).read_bytes())
        for source, destination in {
            "runtime": "bin/grok-bin", "code-mode-host": "bin/codex-code-mode-host",
            "rg": "codex-path/rg", "zsh": "codex-resources/zsh/bin/zsh", "bwrap": "codex-resources/bwrap",
        }.items():
            self.assertEqual((self.inputs / source).read_bytes(), (self.output / destination).read_bytes())
        for name in expected:
            if name.startswith(("bin/", "codex-path/", "codex-resources/")):
                self.assertTrue(os.access(self.output / name, os.X_OK), name)

    def test_mac_staging_requires_helpers_but_has_no_linux_resource(self):
        self.invoke(MAC)
        metadata = json.loads((self.output / "grok-package.json").read_text())
        self.assertEqual(metadata["target"], MAC)
        self.assertNotIn("codex-resources/bwrap", metadata["files"])
        self.assertIn("codex-resources/zsh/bin/zsh", metadata["files"])
        self.assertFalse((self.output / "codex-resources/bwrap").exists())

    def test_rejects_wrong_source_and_target_before_writing(self):
        for extra in (("--source-sha", "a" * 40), ("--source-sha", "missing"), ("--target", "x86_64-apple-darwin")):
            with self.subTest(extra=extra):
                self.invoke(extra=extra, success=False)
                self.assertFalse(self.output.exists())

    def test_rejects_dirty_owned_asset_before_writing(self):
        (self.repo / "grok/dist/models.json").write_text("{}")
        self.invoke(success=False)
        self.assertFalse(self.output.exists())

    def test_rejects_untracked_owned_asset_before_writing(self):
        subprocess.run(["git", "rm", "--cached", "grok/dist/grok"], cwd=self.repo, check=True, capture_output=True)
        subprocess.run(["git", "-c", "user.name=Package fixture", "-c", "user.email=fixture@example.invalid",
                        "commit", "--quiet", "-m", "untrack launcher"], cwd=self.repo, check=True, capture_output=True)
        self.sha = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=self.repo, text=True).strip()
        self.invoke(success=False)
        self.assertFalse(self.output.exists())

    def test_rejects_missing_empty_symlink_or_nonexecutable_input(self):
        path = self.inputs / "rg"
        path.unlink()
        for state in ("missing", "empty", "symlink", "nonexecutable"):
            with self.subTest(state=state):
                if state == "empty":
                    path.touch(mode=0o755)
                elif state == "symlink":
                    path.unlink()
                    path.symlink_to(self.inputs / "runtime")
                elif state == "nonexecutable":
                    path.unlink()
                    path.write_text("not executable")
                self.invoke(success=False)
                self.assertFalse(self.output.exists())

    def test_rejects_inappropriate_linux_helper_for_mac(self):
        self.invoke(MAC, extra=("--bwrap", str(self.inputs / "bwrap")), success=False)
        self.assertFalse(self.output.exists())

    def test_never_overwrites_existing_directory_file_or_symlink(self):
        sentinel = b"unrelated user state"
        self.output.mkdir()
        (self.output / "config.toml").write_bytes(sentinel)
        self.invoke(success=False)
        self.assertEqual((self.output / "config.toml").read_bytes(), sentinel)
        shutil.rmtree(self.output)
        self.output.write_bytes(sentinel)
        self.invoke(success=False)
        self.assertEqual(self.output.read_bytes(), sentinel)
        self.output.unlink()
        self.output.symlink_to(self.inputs, target_is_directory=True)
        self.invoke(success=False)
        self.assertTrue(self.output.is_symlink())
        self.assertEqual(len(list(self.inputs.iterdir())), 5)

    def test_launcher_preserves_arguments_and_dedicated_home_without_writes(self):
        self.invoke()
        user_home = self.root / "user"
        user_home.mkdir()
        dedicated = user_home / "dedicated product"
        dedicated.mkdir()
        config = dedicated / "config.toml"
        config.write_text("user-owned configuration")
        env = dict(os.environ, HOME=str(user_home), CODEX_HOME=str(dedicated))
        args = ["value with spaces", "--help", "$(not-a-command)"]
        result = subprocess.run([str(self.output / "bin/grok"), *args], env=env, capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), [str(dedicated), args])
        self.assertEqual(config.read_text(), "user-owned configuration")
        self.assertEqual(list(dedicated.iterdir()), [config])

    def test_launcher_rejects_shared_alias_missing_or_unconfigured_home(self):
        self.invoke()
        user_home = self.root / "user"
        user_home.mkdir()
        for name in (".codex", ".grok", "unconfigured"):
            (user_home / name).mkdir()
            if name != "unconfigured":
                (user_home / name / "config.toml").write_text("unrelated configuration")
        (user_home / "alias").symlink_to(user_home / ".codex", target_is_directory=True)
        for selected in ("", "relative", str(user_home / "missing"), str(user_home / "unconfigured"),
                         str(user_home / ".codex"), str(user_home / ".grok") + "/../.grok/", str(user_home / "alias")):
            with self.subTest(home=selected):
                env = dict(os.environ, HOME=str(user_home), CODEX_HOME=selected)
                result = subprocess.run([str(self.output / "bin/grok")], env=env, capture_output=True, text=True, timeout=5)
                self.assertEqual(result.returncode, 2)
                self.assertEqual(result.stdout, "")
        self.assertFalse((user_home / "missing").exists())
        for name in (".codex", ".grok"):
            self.assertEqual((user_home / name / "config.toml").read_text(), "unrelated configuration")


if __name__ == "__main__":
    unittest.main()
