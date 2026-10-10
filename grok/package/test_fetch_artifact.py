import copy
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile

from extract_artifact import EXECUTABLES, FILES
from select_artifact import REPOSITORY, TARGET


SHA = "a" * 40
HARNESS = "b" * 40
PRIVATE = "PRIVATE_RESPONSE_CANARY"
NAME = f"grok-{SHA}-{TARGET}-37-2"


class FetchCommandTests(unittest.TestCase):
    """Run the real acquisition command; only the external gh binary is fake."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.output = self.root / "result"
        self.fixture_root = self.root / "fixtures"
        self.fixture_root.mkdir()
        self.log = self.root / "gh-calls.jsonl"
        self.marker = self.root / "PACKAGE_EXECUTED"
        self.contents = {name: (name + " fixture\n").encode() for name in FILES}
        for name in EXECUTABLES:
            self.contents[name] = f"#!/bin/sh\nprintf executed > '{self.marker}'\nexit 91\n".encode()
        self.contents["codex-package.json"] = json.dumps({
            "layoutVersion": 1, "version": "0.158.0", "target": TARGET, "variant": "grok",
            "entrypoint": "bin/grok-bin", "resourcesDir": "codex-resources", "pathDir": "codex-path",
        }).encode()
        self.manifest = {
            "schema_version": 1, "repository": REPOSITORY, "source_sha": SHA, "target": TARGET,
            "version": "0.158.0", "entrypoint": "bin/grok", "runtime": "bin/grok-bin",
            "files": {name: {"size": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                      for name, data in self.contents.items() if name != "grok-package.json"},
        }
        self.contents["grok-package.json"] = json.dumps(self.manifest).encode()
        self.run = {
            "id": 37, "run_attempt": 2, "head_sha": SHA, "status": "completed",
            "conclusion": "failure", "event": "push", "path": ".github/workflows/grok.yml",
            "head_branch": "grok/rust-v0.158.0", "repository": {"full_name": REPOSITORY},
            "head_repository": {"full_name": REPOSITORY}, "private": PRIVATE,
        }
        self.jobs = {"total_count": 2, "jobs": [
            {"id": 101, "run_id": 37, "run_attempt": 2, "name": "Build " + TARGET,
             "status": "completed", "conclusion": "success"},
            {"name": "Build aarch64-apple-darwin", "status": "completed", "conclusion": "failure"},
        ], "private": PRIVATE}
        self.artifacts = {"total_count": 1, "artifacts": [{
            "id": 501, "name": NAME, "expired": False,
            "workflow_run": {"id": 37, "head_sha": SHA},
            "archive_download_url": "https://untrusted.invalid/" + PRIVATE,
        }], "private": PRIVATE}
        self.bundle()
        prefix = f"/repos/{REPOSITORY}/actions"
        self.endpoints = [f"{prefix}/runs/37", f"{prefix}/runs/37/attempts/2/jobs?per_page=100&page=1",
                          f"{prefix}/runs/37/artifacts?per_page=100&page=1", f"{prefix}/artifacts/501/zip"]
        self.expected_calls = [["api", "--hostname", "github.com", "--method", "GET", endpoint]
                               for endpoint in self.endpoints]
        self.config = {"calls": self.expected_calls,
                       "files": ["run.json", "jobs.json", "artifacts.json", "artifact.zip"]}
        fake_bin = self.root / "fake-bin"
        fake_bin.mkdir()
        fake = fake_bin / "gh"
        fake.write_text(f"#!{sys.executable}\n" + r'''
import json
import os
from pathlib import Path
import sys

root = Path(os.environ["FAKE_GH_FIXTURES"])
log = Path(os.environ["FAKE_GH_LOG"])
config = json.loads((root / "config.json").read_text())
index = len(log.read_text().splitlines()) if log.exists() else 0
with log.open("a") as target:
    target.write(json.dumps(sys.argv[1:]) + "\n")
if index >= len(config["calls"]) or sys.argv[1:] != config["calls"][index]:
    print("PRIVATE_RESPONSE_CANARY unexpected gh call", file=sys.stderr)
    raise SystemExit(88)
if config.get("fail_index") == index:
    print("PRIVATE_RESPONSE_CANARY https://signed.invalid/?secret=PRIVATE_RESPONSE_CANARY")
    print("PRIVATE_RESPONSE_CANARY gh authentication error", file=sys.stderr)
    raise SystemExit(17)
if config.get("create_output_index") == index:
    output = Path(config["output"])
    output.mkdir()
    (output / "sentinel").write_text("preserve me")
sys.stdout.buffer.write((root / config["files"][index]).read_bytes())
''')
        fake.chmod(0o755)
        self.env = dict(os.environ, PATH=str(fake_bin) + os.pathsep + os.environ.get("PATH", ""),
                        FAKE_GH_FIXTURES=str(self.fixture_root), FAKE_GH_LOG=str(self.log),
                        GH_HOST="untrusted.invalid", GH_TOKEN="SYNTHETIC_TEST_TOKEN")

    def bundle(self, contents=None):
        body = io.BytesIO()
        with tarfile.open(fileobj=body, mode="w:gz") as archive:
            for name, data in (self.contents if contents is None else contents).items():
                info = tarfile.TarInfo("./" + name)
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))
        self.tar_digest = hashlib.sha256(body.getvalue()).hexdigest()
        archive = self.fixture_root / "artifact.zip"
        with zipfile.ZipFile(archive, "w") as zipped:
            zipped.writestr(f"grok-{SHA}-{TARGET}.tar.gz", body.getvalue())
        self.artifacts["artifacts"][0].update(
            digest="sha256:" + hashlib.sha256(archive.read_bytes()).hexdigest(),
            size_in_bytes=archive.stat().st_size)

    def invoke(self, extra=(), harness=HARNESS):
        for name, body in (("run", self.run), ("jobs", self.jobs),
                           ("artifacts", self.artifacts), ("config", self.config)):
            (self.fixture_root / (name + ".json")).write_text(json.dumps(body))
        result = subprocess.run(
            [sys.executable, str(Path(__file__).with_name("fetch_artifact.py")),
             "--run-id", "37", "--harness-sha", harness, "--output", str(self.output), *extra],
            capture_output=True, text=True, timeout=10, env=self.env,
        )
        self.assertFalse(self.marker.exists(), "retrieval must never execute package bytes")
        self.assertNotIn(PRIVATE, result.stdout + result.stderr)
        self.assertNotIn("SYNTHETIC_TEST_TOKEN", result.stdout + result.stderr)
        self.assertEqual(list(self.root.glob(".grok-artifact-*")), [])
        return result

    def assert_calls(self, count):
        calls = [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []
        self.assertEqual(calls, self.expected_calls[:count])

    def assert_failed(self, result, calls, output_absent=True):
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr,
                         "Package artifact acquisition failed; no package execution was attempted.\n")
        self.assert_calls(calls)
        if output_absent:
            self.assertFalse(self.output.exists())

    def test_diagnostic_command_fetches_exact_id_and_returns_only_verified_metadata(self):
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        self.assert_calls(4)
        expected = {
            "repository": REPOSITORY, "run_id": 37, "run_attempt": 2,
            "source_sha": SHA, "target": TARGET, "harness_sha": HARNESS,
            "artifact_id": 501, "artifact_name": NAME,
            "artifact_digest": self.artifacts["artifacts"][0]["digest"],
            "artifact_size": self.artifacts["artifacts"][0]["size_in_bytes"],
            "producer_job_id": 101, "selection_mode": "diagnostic", "run_status": "completed",
            "run_conclusion": "failure", "package_archive_sha256": self.tar_digest,
            "runtime_sha256": self.manifest["files"]["bin/grok-bin"]["sha256"],
        }
        self.assertEqual(json.loads(result.stdout), expected)
        self.assertEqual(json.loads((self.output / "verified.json").read_text()), expected)
        selected = {key: value for key, value in expected.items()
                    if key not in ("package_archive_sha256", "runtime_sha256")}
        self.assertEqual(json.loads((self.output / "selection.json").read_text()), selected)
        self.assertEqual({path.name for path in self.output.iterdir()}, {"package", "selection.json", "verified.json"})
        self.assertEqual({p.relative_to(self.output / "package").as_posix()
                          for p in (self.output / "package").rglob("*") if p.is_file()}, FILES)
        for name, data in self.contents.items():
            path = self.output / "package" / name
            self.assertEqual(path.read_bytes(), data)
            self.assertEqual(path.stat().st_mode & 0o7777, 0o755 if name in EXECUTABLES else 0o600)

    def test_same_run_command_binds_source_harness_attempt_and_artifact(self):
        self.run.update(status="in_progress", conclusion=None)
        result = self.invoke(["--same-run-source-sha", SHA, "--expected-artifact-id", "501",
                              "--expected-run-attempt", "2"], harness=SHA)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_calls(4)
        got = json.loads(result.stdout)
        self.assertEqual(got["selection_mode"], "same_run")
        self.assertEqual(got["harness_sha"], got["source_sha"])
        self.assertEqual(got["run_attempt"], 2)
        self.assertEqual(got["artifact_id"], 501)
        self.assertEqual(got["run_status"], "in_progress")
        self.assertIsNone(got["run_conclusion"])

    def test_wrong_same_run_expected_identity_never_downloads(self):
        self.run.update(status="in_progress", conclusion=None)
        for artifact, attempt in (("502", "2"), ("501", "3")):
            with self.subTest(artifact=artifact, attempt=attempt):
                self.log.unlink(missing_ok=True)
                result = self.invoke(["--same-run-source-sha", SHA, "--expected-artifact-id", artifact,
                                      "--expected-run-attempt", attempt], harness=SHA)
                self.assert_failed(result, 3)

    def test_incomplete_same_run_arguments_make_no_external_calls(self):
        self.assert_failed(self.invoke(["--same-run-source-sha", SHA], harness=SHA), 0)

    def test_wrong_harness_same_run_binding_makes_no_external_calls(self):
        self.assert_failed(self.invoke(["--same-run-source-sha", SHA, "--expected-artifact-id", "501",
                                        "--expected-run-attempt", "2"]), 0)

    def test_wrong_run_workflow_or_fork_never_downloads(self):
        for key, value in (("id", 38), ("path", ".github/workflows/other.yml"),
                           ("head_repository", {"full_name": "untrusted/fork"})):
            with self.subTest(field=key):
                original = self.run[key]
                self.run[key] = value
                self.log.unlink(missing_ok=True)
                self.assert_failed(self.invoke(), 3)
                self.run[key] = original

    def test_wrong_run_attempt_cannot_enter_an_endpoint(self):
        self.run["run_attempt"] = "2/" + PRIVATE
        self.assert_failed(self.invoke(), 1)

    def test_incomplete_jobs_inventory_never_downloads_or_falls_back(self):
        self.jobs["total_count"] = 101
        self.assert_failed(self.invoke(), 3)

    def test_incomplete_artifacts_inventory_never_downloads_or_falls_back(self):
        self.artifacts["total_count"] = 101
        self.assert_failed(self.invoke(), 3)

    def test_unfinished_build_never_downloads(self):
        self.jobs["jobs"][0].update(status="in_progress", conclusion=None)
        self.assert_failed(self.invoke(), 3)

    def test_ambiguous_artifact_never_downloads(self):
        self.artifacts["artifacts"].append(copy.deepcopy(self.artifacts["artifacts"][0]))
        self.artifacts["total_count"] = 2
        self.assert_failed(self.invoke(), 3)

    def test_failed_gh_at_any_stage_has_safe_error_and_no_extra_calls(self):
        for index in range(4):
            with self.subTest(index=index):
                self.log.unlink(missing_ok=True)
                self.config["fail_index"] = index
                self.assert_failed(self.invoke(), index + 1)

    def test_invalid_json_is_not_echoed_or_followed_by_another_call(self):
        (self.fixture_root / "invalid.json").write_text(PRIVATE)
        self.config["files"][0] = "invalid.json"
        self.assert_failed(self.invoke(), 1)

    def test_deeply_nested_json_is_a_safe_failure(self):
        (self.fixture_root / "invalid.json").write_text('{"private":' + '[' * 2000 +
                                                       '"' + PRIVATE + '"' + ']' * 2000 + '}')
        self.config["files"][0] = "invalid.json"
        self.assert_failed(self.invoke(), 1)

    def test_missing_gh_is_a_safe_failure_without_setup_or_fallback(self):
        self.env["PATH"] = str(self.root / "no-tools")
        self.assert_failed(self.invoke(), 0)

    def test_mutated_download_is_rejected_without_publishing_package(self):
        archive = self.fixture_root / "artifact.zip"
        data = bytearray(archive.read_bytes())
        data[-1] ^= 1
        archive.write_bytes(data)
        self.assert_failed(self.invoke(), 4)

    def test_archive_over_advertised_size_is_bounded_and_rejected(self):
        self.artifacts["artifacts"][0]["size_in_bytes"] = 1
        self.assert_failed(self.invoke(), 4)

    def test_mutated_runtime_cannot_publish_or_execute(self):
        contents = dict(self.contents)
        contents["bin/grok-bin"] += b"modified"
        self.bundle(contents)
        self.assert_failed(self.invoke(), 4)

    def test_unsupported_or_corrupt_zip_compression_has_a_safe_error(self):
        for method in (99, 8):
            with self.subTest(method=method):
                self.bundle()
                archive = self.fixture_root / "artifact.zip"
                data = bytearray(archive.read_bytes())
                central = data.index(b"PK\x01\x02")
                data[8:10] = method.to_bytes(2, "little")
                data[central + 10:central + 12] = method.to_bytes(2, "little")
                archive.write_bytes(data)
                self.artifacts["artifacts"][0]["digest"] = "sha256:" + hashlib.sha256(data).hexdigest()
                self.log.unlink(missing_ok=True)
                self.assert_failed(self.invoke(), 4)

    def test_existing_destination_is_preserved_without_any_external_calls(self):
        self.output.mkdir()
        sentinel = self.output / "sentinel"
        sentinel.write_text("preserve me")
        self.assert_failed(self.invoke(), 0, output_absent=False)
        self.assertEqual(list(self.output.iterdir()), [sentinel])
        self.assertEqual(sentinel.read_text(), "preserve me")

    def test_destination_created_during_download_is_preserved(self):
        self.config.update(create_output_index=3, output=str(self.output))
        self.assert_failed(self.invoke(), 4, output_absent=False)
        self.assertEqual([path.name for path in self.output.iterdir()], ["sentinel"])
        self.assertEqual((self.output / "sentinel").read_text(), "preserve me")

    def test_broken_destination_symlink_is_preserved_without_external_calls(self):
        self.output.symlink_to(self.root / "absent")
        self.assert_failed(self.invoke(), 0)
        self.assertTrue(self.output.is_symlink())


if __name__ == "__main__":
    unittest.main()
