import copy
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile

from extract_artifact import EXECUTABLES, FILES, extract
from select_artifact import REPOSITORY, TARGET


SHA = "a" * 40


class ExtractTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.output = self.root / "unpacked"
        self.contents = {name: (name + " fixture\n").encode() for name in FILES}
        self.contents["codex-package.json"] = json.dumps({
            "layoutVersion": 1, "version": "0.158.0", "target": TARGET, "variant": "grok",
            "entrypoint": "bin/grok-bin", "resourcesDir": "codex-resources", "pathDir": "codex-path",
        }).encode()
        self.metadata = {
            "schema_version": 1, "repository": REPOSITORY, "source_sha": SHA, "target": TARGET,
            "version": "0.158.0", "entrypoint": "bin/grok", "runtime": "bin/grok-bin",
            "files": {name: {"size": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                      for name, data in self.contents.items() if name != "grok-package.json"},
        }
        self.contents["grok-package.json"] = json.dumps(self.metadata).encode()

    def bundle(self, contents=None, extra=None, zip_name=None):
        body = io.BytesIO()
        with tarfile.open(fileobj=body, mode="w:gz") as archive:
            for name, data in (self.contents if contents is None else contents).items():
                info = tarfile.TarInfo("./" + name)
                info.size = len(data)
                info.mode = 0o7777  # Extraction chooses safe modes, never these.
                archive.addfile(info, io.BytesIO(data))
            if extra is not None:
                archive.addfile(extra, io.BytesIO(b"x" * extra.size))
        tar_bytes = body.getvalue()
        path = self.root / "artifact.zip"
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr(zip_name or f"grok-{SHA}-{TARGET}.tar.gz", tar_bytes)
        selected = {"repository": REPOSITORY, "target": TARGET, "source_sha": SHA,
                    "artifact_digest": "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest(),
                    "artifact_size": path.stat().st_size}
        return path, selected, hashlib.sha256(tar_bytes).hexdigest()

    def test_exact_archive_and_all_file_digests_before_executable_permissions(self):
        path, selected, tar_digest = self.bundle()
        got = extract(path, selected, self.output)
        self.assertEqual(got, {**selected, "package_archive_sha256": tar_digest,
                               "runtime_sha256": self.metadata["files"]["bin/grok-bin"]["sha256"]})
        self.assertEqual({p.relative_to(self.output).as_posix() for p in self.output.rglob("*") if p.is_file()}, FILES)
        for name, data in self.contents.items():
            self.assertEqual((self.output / name).read_bytes(), data)
            self.assertEqual((self.output / name).stat().st_mode & 0o7777, 0o755 if name in EXECUTABLES else 0o600)

    def test_wrong_archive_identity_fails_before_creating_output(self):
        path, selected, _ = self.bundle()
        for key, value in (("source_sha", "b" * 40), ("target", "aarch64-apple-darwin"),
                           ("artifact_digest", "sha256:" + "b" * 64), ("artifact_size", 1)):
            with self.subTest(field=key):
                changed = dict(selected, **{key: value})
                with self.assertRaises(ValueError):
                    extract(path, changed, self.output)
                self.assertFalse(self.output.exists())

    def test_wrong_zip_member_name_is_not_extracted(self):
        path, selected, _ = self.bundle(zip_name="../escaped.tar.gz")
        with self.assertRaises(ValueError):
            extract(path, selected, self.output)
        self.assertFalse(self.output.exists())

    def test_tar_traversal_symlink_duplicate_and_extra_members_are_rejected(self):
        for kind in ("traversal", "symlink", "duplicate", "extra"):
            with self.subTest(kind=kind):
                name = {"traversal": "../escaped", "symlink": "bin/link", "duplicate": "bin/grok-bin", "extra": "secret.txt"}[kind]
                extra = tarfile.TarInfo(name)
                extra.size = 1
                if kind == "symlink":
                    extra.type = tarfile.SYMTYPE
                    extra.linkname = "/outside"
                    extra.size = 0
                path, selected, _ = self.bundle(extra=extra)
                with self.assertRaises(ValueError):
                    extract(path, selected, self.output)
                self.assertFalse(self.output.exists())
                self.assertFalse((self.root / "escaped").exists())

    def test_missing_helper_is_not_a_complete_package(self):
        contents = dict(self.contents)
        del contents["codex-resources/bwrap"]
        path, selected, _ = self.bundle(contents)
        with self.assertRaises(ValueError):
            extract(path, selected, self.output)
        self.assertFalse(self.output.exists())

    def test_mutated_bytes_or_manifest_cannot_become_executable(self):
        for kind in ("runtime", "source", "target", "layout"):
            with self.subTest(kind=kind):
                contents = dict(self.contents)
                metadata = copy.deepcopy(self.metadata)
                if kind == "runtime":
                    contents["bin/grok-bin"] = b"modified binary"
                elif kind == "layout":
                    layout = json.loads(contents["codex-package.json"])
                    layout["resourcesDir"] = "elsewhere"
                    contents["codex-package.json"] = json.dumps(layout).encode()
                    metadata["files"]["codex-package.json"] = {"size": len(contents["codex-package.json"]),
                        "sha256": hashlib.sha256(contents["codex-package.json"]).hexdigest()}
                else:
                    metadata["source_sha" if kind == "source" else "target"] = "wrong"
                contents["grok-package.json"] = json.dumps(metadata).encode()
                path, selected, _ = self.bundle(contents)
                destination = self.root / kind
                with self.assertRaises(ValueError):
                    extract(path, selected, destination)
                self.assertEqual((destination / "bin/grok-bin").stat().st_mode & 0o111, 0)

    def test_existing_output_is_preserved(self):
        path, selected, _ = self.bundle()
        self.output.mkdir()
        sentinel = self.output / "config.toml"
        sentinel.write_text("unrelated home")
        with self.assertRaises(FileExistsError):
            extract(path, selected, self.output)
        self.assertEqual(list(self.output.iterdir()), [sentinel])
        self.assertEqual(sentinel.read_text(), "unrelated home")

    def test_real_command_reports_verified_package_or_nonzero(self):
        path, selected, _ = self.bundle()
        metadata = self.root / "selection.json"
        metadata.write_text(json.dumps(selected))
        args = [sys.executable, str(Path(__file__).with_name("extract_artifact.py")), "--selection-json", str(metadata),
                "--archive", str(path), "--output", str(self.output)]
        result = subprocess.run(args, capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["runtime_sha256"], self.metadata["files"]["bin/grok-bin"]["sha256"])
        failed = subprocess.run(args, capture_output=True, text=True, timeout=5)
        self.assertNotEqual(failed.returncode, 0)
        self.assertEqual(failed.stdout, "")


if __name__ == "__main__":
    unittest.main()
