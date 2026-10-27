import copy
from contextlib import redirect_stderr, redirect_stdout
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import time
import tempfile
import textwrap
import unittest
from unittest import mock
import zipfile
from urllib.parse import urlencode

import fetch_artifact
import select_artifact
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
            "layoutVersion": 1, "version": "0.164.0", "target": TARGET, "variant": "grok",
            "entrypoint": "bin/grok-bin", "resourcesDir": "codex-resources", "pathDir": "codex-path",
        }).encode()
        self.manifest = {
            "schema_version": 1, "repository": REPOSITORY, "source_sha": SHA, "target": TARGET,
            "version": "0.164.0", "entrypoint": "bin/grok", "runtime": "bin/grok-bin",
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
        self.artifact = {"id": 501, "name": NAME, "expired": False,
            "workflow_run": {"id": 37, "head_sha": SHA},
            "archive_download_url": "https://untrusted.invalid/" + PRIVATE,
            "private": PRIVATE}
        self.bundle()
        prefix = f"/repos/{REPOSITORY}/actions"
        self.endpoints = [f"{prefix}/artifacts/501", f"{prefix}/runs/37",
                          f"{prefix}/runs/37/attempts/2/jobs?per_page=100&page=1", f"{prefix}/artifacts/501/zip"]
        self.expected_calls = [["api", "--hostname", "github.com", "--method", "GET", endpoint]
                               for endpoint in self.endpoints]
        self.config = {"calls": self.expected_calls,
                       "files": ["artifact.json", "run.json", "jobs.json", "artifact.zip"]}
        fake_bin = self.root / "fake-bin"
        fake_bin.mkdir()
        fake = fake_bin / "gh"
        fake.write_text(f"#!{sys.executable}\n" + r'''
import json
import os
from pathlib import Path
import sys
import time

root = Path(os.environ["FAKE_GH_FIXTURES"])
log = Path(os.environ["FAKE_GH_LOG"])
config = json.loads((root / "config.json").read_text())
index = len(log.read_text().splitlines()) if log.exists() else 0
with log.open("a") as target:
    target.write(json.dumps(sys.argv[1:]) + "\n")
if index >= len(config["calls"]) or sys.argv[1:] != config["calls"][index]:
    print("PRIVATE_RESPONSE_CANARY unexpected gh call", file=sys.stderr)
    raise SystemExit(88)
if "stderr" in config:
    data = config["stderr"].encode()
    for _ in range(config.get("stderr_repeat", 1)):
        sys.stderr.buffer.write(data)
    sys.stderr.buffer.flush()
if config.get("stream_stderr"):
    while True:
        sys.stderr.buffer.write(b"PRIVATE_RESPONSE_CANARY" * 1024)
if config.get("sleep_index") == index:
    for fd in config.get("close_fds", []):
        os.close(fd)
    time.sleep(30)
if config.get("fail_index") == index:
    print("PRIVATE_RESPONSE_CANARY https://signed.invalid/?secret=PRIVATE_RESPONSE_CANARY")
    if "stderr" not in config:
        print("PRIVATE_RESPONSE_CANARY gh authentication error", file=sys.stderr)
    raise SystemExit(17)
if config.get("create_output_index") == index:
    output = Path(config["output"])
    output.mkdir()
    (output / "sentinel").write_text("preserve me")
sys.stdout.buffer.write((root / config["files"][index]).read_bytes())
''')
        fake.chmod(0o755)
        git = fake_bin / "git"
        git.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$FAKE_GIT_LOG\"\n"
                       "printf PRIVATE_RESPONSE_CANARY >&2\n"
                       "case \"$1\" in\n rev-parse) printf '%s\\n' \"$FAKE_MERGE_SHA\";;\n"
                       " merge-base) exit \"${FAKE_GIT_EXIT:-0}\";;\n"
                       " diff) printf '%s' \"${FAKE_DIFF:-}\";;\nesac\n")
        git.chmod(0o755)
        self.env = dict(os.environ, PATH=str(fake_bin) + os.pathsep + os.environ.get("PATH", ""),
                        FAKE_GH_FIXTURES=str(self.fixture_root), FAKE_GH_LOG=str(self.log),
                        GH_HOST="untrusted.invalid", GH_TOKEN="SYNTHETIC_TEST_TOKEN",
                        FAKE_GIT_LOG=str(self.root / "git-calls"), FAKE_MERGE_SHA=SHA)

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
        self.artifact.update(
            digest="sha256:" + hashlib.sha256(archive.read_bytes()).hexdigest(),
            size_in_bytes=archive.stat().st_size)

    def prepare_fixtures(self):
        for name, body in (("run", self.run), ("jobs", self.jobs),
                           ("artifact", self.artifact), ("config", self.config)):
            (self.fixture_root / (name + ".json")).write_text(json.dumps(body))

    def invoke(self, extra=(), harness=HARNESS):
        self.prepare_fixtures()
        result = subprocess.run(
            [sys.executable, str(Path(__file__).with_name("fetch_artifact.py")),
             "--artifact-id", "501", "--harness-sha", harness, "--output", str(self.output), *extra],
            capture_output=True, text=True, timeout=10, env=self.env,
        )
        self.assertFalse(self.marker.exists(), "retrieval must never execute package bytes")
        self.assertNotIn(PRIVATE, result.stdout + result.stderr)
        self.assertNotIn("SYNTHETIC_TEST_TOKEN", result.stdout + result.stderr)
        self.assertNotIn("https://", result.stdout + result.stderr)
        self.assertEqual(list(self.root.glob(".grok-artifact-*")), [])
        return result

    def assert_calls(self, count):
        calls = [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []
        self.assertEqual(calls, self.expected_calls[:count])

    def assert_failed(self, result, calls, code, output_absent=True, summary=None, selection_reason=None):
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "")
        detail = f"; {summary}" if summary else ""
        if selection_reason:
            detail += f"; selection={selection_reason}"
        self.assertEqual(result.stderr,
                         f"Package artifact acquisition failed [{code}]{detail}; no package execution was attempted.\n")
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
            "source_sha": SHA, "producer_line": "grok/rust-v0.158.0", "target": TARGET, "harness_sha": HARNESS,
            "artifact_id": 501, "artifact_name": NAME,
            "artifact_digest": self.artifact["digest"],
            "artifact_size": self.artifact["size_in_bytes"],
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

    def configure_pr(self, canonical=False):
        ci_id = 38 if canonical else 37
        merge_sha = "c" * 40 if canonical else SHA
        self.env["FAKE_MERGE_SHA"] = merge_sha
        repo = {"id": 123, "full_name": REPOSITORY}
        pr = {"number": 444, "state": "open", "merged": False, "merge_commit_sha": merge_sha,
              "head": {"sha": HARNESS, "ref": "fix/image", "repo": dict(repo)},
              "base": {"sha": "d" * 40, "ref": "grok/rust-v0.164.0", "repo": dict(repo)}}
        ci = {**self.run, "id": ci_id, "event": "pull_request", "head_sha": HARNESS, "head_branch": "fix/image",
              "conclusion": "success", "run_attempt": 3, "pull_requests": [copy.deepcopy(pr)]}
        if canonical:
            self.run["head_branch"] = pr["base"]["ref"]
        else:
            self.run.update(ci)
            self.artifact["workflow_run"]["head_sha"] = HARNESS
        prefix = f"/repos/{REPOSITORY}"
        requests = [
            (prefix + "/pulls?" + urlencode({"state": "open", "head": "Harness-X-Harness:fix/image", "per_page": 100, "page": 1}), "pulls.json"),
            (prefix + "/pulls/444", "pull-request.json"),
            (prefix + "/actions/workflows/grok.yml/runs?" + urlencode({"event": "pull_request", "head_sha": HARNESS, "per_page": 1, "page": 1}), "harness-ci.json"),
            (prefix + f"/actions/runs/{ci_id}/artifacts?per_page=100&page=1", "harness-artifacts.json"),
            (prefix + f"/actions/runs/{ci_id}/attempts/1/jobs?per_page=100&page=1", "harness-jobs.json"),
            (prefix + "/actions/artifacts/901/zip", "harness-subject.zip"),
        ]
        for index, (endpoint, filename) in enumerate(requests, 2):
            self.expected_calls.insert(index, ["api", "--hostname", "github.com", "--method", "GET", endpoint])
            self.config["files"].insert(index, filename)
        for name, body in (("pulls", [{"number": 444}]), ("pull-request", pr),
                           ("harness-ci", {"total_count": 1, "workflow_runs": [ci]})):
            (self.fixture_root / (name + ".json")).write_text(json.dumps(body))
        report = {
            "GITHUB_REPOSITORY": REPOSITORY, "GITHUB_RUN_ID": str(ci_id), "GITHUB_RUN_ATTEMPT": "1",
            "GITHUB_EVENT_NAME": "pull_request", "GITHUB_REF": "refs/pull/444/merge",
            "GITHUB_SHA": merge_sha, "GROK_CHECKOUT_SHA": merge_sha, "GROK_SUBJECT_KIND": "pr_test_merge",
            "GITHUB_WORKFLOW_REF": REPOSITORY + "/.github/workflows/grok.yml@refs/pull/444/merge",
            "GITHUB_WORKFLOW_SHA": merge_sha, "PR_HEAD_SHA": HARNESS, "PR_BASE_SHA": pr["base"]["sha"],
        }
        archive = self.fixture_root / "harness-subject.zip"
        with zipfile.ZipFile(archive, "w") as zipped:
            zipped.writestr("grok-subject.json", json.dumps(report))
        subjects = {"total_count": 1, "artifacts": [{"id": 901, "name": f"grok-subject-{merge_sha}-{ci_id}-1",
            "expired": False, "size_in_bytes": archive.stat().st_size,
            "digest": "sha256:" + hashlib.sha256(archive.read_bytes()).hexdigest(),
            "workflow_run": {"id": ci_id, "head_sha": HARNESS}}]}
        subject_jobs = {"total_count": 1, "jobs": [{"id": 902, "name": "Native proof / Execution subject",
            "run_id": ci_id, "run_attempt": 1, "status": "completed", "conclusion": "success"}]}
        for name, body in (("harness-artifacts", subjects), ("harness-jobs", subject_jobs)):
            (self.fixture_root / (name + ".json")).write_text(json.dumps(body))
        if not canonical:
            producer_requests = [
                (prefix + "/actions/runs/37/artifacts?per_page=100&page=1", "harness-artifacts.json"),
                (prefix + "/actions/runs/37/attempts/1/jobs?per_page=100&page=1", "harness-jobs.json"),
                (prefix + "/actions/artifacts/901/zip", "harness-subject.zip"),
                (prefix + "/pulls/444", "producer-pr.json"),
            ]
            for index, (endpoint, filename) in enumerate(producer_requests, 9):
                self.expected_calls.insert(index, ["api", "--hostname", "github.com", "--method", "GET", endpoint])
                self.config["files"].insert(index, filename)
            (self.fixture_root / "producer-pr.json").write_text(json.dumps(pr))
        return pr

    def test_missing_subject_is_unavailable_but_explicit_consumer_rejects_it(self):
        self.configure_pr()
        (self.fixture_root / "harness-artifacts.json").write_text(json.dumps(
            {"total_count": 0, "artifacts": []}))
        self.assert_failed(self.invoke(["--harness-ref", "fix/image"]),
                            6, "harness_ci_metadata")

    def test_subject_availability_does_not_hide_identity_or_producer_failures(self):
        self.configure_pr()
        original = json.loads((self.fixture_root / "harness-artifacts.json").read_text())
        def read_fixture(endpoint, destination, limit):
            name = "harness-jobs.json" if "/jobs?" in endpoint else "harness-artifacts.json"
            destination.write_bytes((self.fixture_root / name).read_bytes())
        cases = ("missing", "expired", "wrong_source", "incomplete", "failed_owner")
        for case in cases:
            with self.subTest(case=case), tempfile.TemporaryDirectory() as temporary:
                body = copy.deepcopy(original)
                if case == "missing": body = {"total_count": 0, "artifacts": []}
                if case == "expired": body["artifacts"][0]["expired"] = True
                if case == "wrong_source": body["artifacts"][0]["name"] = body["artifacts"][0]["name"].replace(SHA, "e" * 40)
                if case == "incomplete": body["total_count"] += 1
                if case == "failed_owner":
                    jobs = json.loads((self.fixture_root / "harness-jobs.json").read_text())
                    jobs["jobs"][0]["conclusion"] = "failure"
                    (self.fixture_root / "harness-jobs.json").write_text(json.dumps(jobs))
                (self.fixture_root / "harness-artifacts.json").write_text(json.dumps(body))
                with mock.patch.object(fetch_artifact, "gh_api", side_effect=read_fixture):
                    with self.assertRaises(select_artifact.SelectionFailure) as error:
                        fetch_artifact.verify_subject(Path(temporary) / "proof", self.run, SHA, None, HARNESS)
                self.assertEqual(isinstance(error.exception, fetch_artifact.ProducerEvidenceUnavailable),
                                 case in ("missing", "expired"))

    def test_pr_command_fetches_current_metadata_and_verifies_merge_package(self):
        self.configure_pr()
        result = self.invoke(["--harness-ref", "fix/image"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_calls(14)
        got = json.loads(result.stdout)
        self.assertEqual(got["selection_mode"], "pr_diagnostic")
        self.assertEqual(got["source_sha"], SHA)
        self.assertEqual(got["harness_sha"], HARNESS)
        self.assertEqual(got["pr_head_sha"], HARNESS)
        self.assertEqual(got["harness_ci_attempt"], 1)
        self.assertEqual(got["harness_ci_artifact_id"], 901)
        self.assertEqual(got["harness_ci_source_sha"], SHA)
        self.assertEqual(got["pull_request_number"], 444)
        self.assertEqual(got["runtime_sha256"], self.manifest["files"]["bin/grok-bin"]["sha256"])

    def test_pr_command_rejects_package_manifest_with_head_instead_of_merge_source(self):
        self.configure_pr()
        self.manifest["source_sha"] = HARNESS
        self.contents["grok-package.json"] = json.dumps(self.manifest).encode()
        self.bundle()
        self.assert_failed(self.invoke(["--harness-ref", "fix/image"]), 14, "archive_verification")

    def test_pr_command_rejects_changed_head_before_archive_download(self):
        pr = self.configure_pr()
        pr["head"]["sha"] = "e" * 40
        (self.fixture_root / "pull-request.json").write_text(json.dumps(pr))
        self.assert_failed(self.invoke(["--harness-ref", "fix/image"]), 5, "harness_ci_metadata")

    def test_pr_harness_accepts_independent_canonical_binary(self):
        self.configure_pr(canonical=True)
        result = self.invoke(["--harness-ref", "fix/image"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_calls(10)
        got = json.loads(result.stdout)
        self.assertEqual(got["source_sha"], SHA)
        self.assertEqual(got["run_id"], 37)
        self.assertEqual(got["harness_ci_run_id"], 38)
        self.assertEqual(got["pr_head_sha"], HARNESS)
        self.assertEqual(got["harness_ci_attempt"], 1)
        self.assertEqual(got["harness_ci_artifact_id"], 901)
        self.assertEqual(got["harness_ci_source_sha"], "c" * 40)
        calls = (self.root / "git-calls").read_text().splitlines()
        self.assertIn("refs/pull/444/merge", calls)
        self.assertNotIn("--is-ancestor", calls)
        self.assertEqual(calls[calls.index("-z") + 1:][:2], [SHA, "c" * 40])

    def test_pr_harness_rejects_binary_diff_before_download(self):
        self.configure_pr(canonical=True)
        self.env["FAKE_DIFF"] = "codex-rs/core/src/lib.rs"
        self.assert_failed(self.invoke(["--harness-ref", "fix/image"]), 9,
                           "selection", selection_reason="identity")

    def test_pr_harness_rejects_changed_merge_object_before_download(self):
        self.configure_pr(canonical=True)
        self.env["FAKE_MERGE_SHA"] = "e" * 40
        self.assert_failed(self.invoke(["--harness-ref", "fix/image"]), 8,
                           "selection", selection_reason="identity")

    def test_historical_subject_rejects_advanced_base_despite_updated_api_links(self):
        pr = self.configure_pr(canonical=True)
        pr["base"]["sha"] = "e" * 40
        (self.fixture_root / "pull-request.json").write_text(json.dumps(pr))
        path = self.fixture_root / "harness-ci.json"
        ci = json.loads(path.read_text())
        ci["workflow_runs"][0]["pull_requests"][0]["base"]["sha"] = pr["base"]["sha"]
        path.write_text(json.dumps(ci))
        self.assert_failed(self.invoke(["--harness-ref", "fix/image"]), 8, "harness_ci_metadata")

    def test_current_harness_merge_must_have_actual_ci_subject_proof(self):
        pr = self.configure_pr(canonical=True)
        pr["merge_commit_sha"] = "e" * 40
        (self.fixture_root / "pull-request.json").write_text(json.dumps(pr))
        self.assert_failed(self.invoke(["--harness-ref", "fix/image"]), 6, "harness_ci_metadata")

    def test_pr_mode_cannot_be_combined_with_formal_source(self):
        self.assert_failed(self.invoke(["--harness-ref", "fix/image", "--expected-line", "grok/rust-v0.164.0"]), 0, "input")

    def test_pr_metadata_request_failure_is_bounded_and_has_no_archive_fetch(self):
        self.configure_pr()
        self.config["fail_index"] = 2
        self.assert_failed(self.invoke(["--harness-ref", "fix/image"]), 3, "pull_request_request")

    def test_original_package_is_fetchable_while_later_attempt_and_sibling_are_running(self):
        self.run.update(status="in_progress", conclusion=None, run_attempt=3)
        self.jobs["jobs"][1].update(status="in_progress", conclusion=None)
        result = self.invoke(["--expected-line", "grok/rust-v0.158.0"], harness=HARNESS)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_calls(4)
        got = json.loads(result.stdout)
        self.assertEqual(got["selection_mode"], "line_bound")
        self.assertEqual(got["harness_sha"], HARNESS)
        self.assertEqual(got["source_sha"], SHA)
        self.assertEqual(got["run_attempt"], 2)
        self.assertEqual(got["artifact_id"], 501)
        self.assertEqual(got["run_status"], "in_progress")
        self.assertIsNone(got["run_conclusion"])
        self.assertEqual((self.root / "git-calls").read_text().splitlines(),
                         ["cat-file", "-e", SHA + "^{commit}",
                          "diff", "--name-only", "--no-renames", "-z", SHA, HARNESS, "--"])

    def test_closed_pr_producer_is_independent_of_current_pr_state_and_consumer_sha(self):
        pr = self.configure_pr()
        # This is a version-line consumer, not an admission of the old PR harness.
        del self.expected_calls[2:8]
        del self.config["files"][2:8]
        self.run["pull_requests"] = []
        pr.update(state="closed", merged=True, merge_commit_sha="f" * 40)
        pr["head"]["sha"] = "e" * 40
        (self.fixture_root / "producer-pr.json").write_text(json.dumps(pr))
        self.env["FAKE_GIT_EXIT"] = "1"  # No ancestry command may decide equivalence.
        result = self.invoke(["--expected-line", "grok/rust-v0.164.0"], harness="e" * 40)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_calls(8)
        got = json.loads(result.stdout)
        self.assertEqual(got["source_sha"], SHA)
        self.assertEqual(got["producer_source"]["head_sha"], HARNESS)
        self.assertEqual(got["harness_sha"], "e" * 40)
        self.assertNotIn("--is-ancestor", (self.root / "git-calls").read_text())

    def test_explicit_fetch_rejects_outside_version_line_before_download(self):
        self.run["head_branch"] = "grok/c7-web-activity-20261008"
        self.assert_failed(self.invoke(), 3, "selection", selection_reason="identity")

    def test_wrong_formal_line_never_downloads(self):
        self.assert_failed(self.invoke(["--expected-line", "grok/rust-v0.164.0"]), 3, "selection", selection_reason="identity")

    def test_wrong_run_workflow_or_fork_never_downloads(self):
        for key, value in (("id", 38), ("path", ".github/workflows/other.yml"),
                           ("head_repository", {"full_name": "untrusted/fork"})):
            with self.subTest(field=key):
                original = self.run[key]
                self.run[key] = value
                self.log.unlink(missing_ok=True)
                self.assert_failed(self.invoke(), 3, "selection", selection_reason="identity")
                self.run[key] = original

    def test_malformed_artifact_attempt_cannot_enter_an_endpoint(self):
        self.artifact["name"] = NAME + "/" + PRIVATE
        self.assert_failed(self.invoke(), 1, "selection", selection_reason="artifact")

    def test_incomplete_jobs_inventory_never_downloads_or_falls_back(self):
        self.jobs["total_count"] = 101
        self.assert_failed(self.invoke(), 3, "selection", selection_reason="jobs_inventory")

    def test_unfinished_build_never_downloads(self):
        self.jobs["jobs"][0].update(status="in_progress", conclusion=None)
        self.assert_failed(self.invoke(), 3, "selection", selection_reason="producer")

    def test_wrong_artifact_id_never_downloads_or_falls_back(self):
        self.artifact["id"] = 502
        self.assert_failed(self.invoke(), 1, "selection", selection_reason="artifact")

    def test_failed_gh_at_any_stage_has_safe_error_and_no_extra_calls(self):
        for index, code in enumerate(("artifact_request", "run_request", "jobs_request", "archive_request")):
            with self.subTest(index=index):
                self.log.unlink(missing_ok=True)
                self.config["fail_index"] = index
                self.assert_failed(self.invoke(), index + 1, code)

    def test_http_error_summaries_at_every_request_do_not_repeat_requests(self):
        for index, code in enumerate(("artifact_request", "run_request", "jobs_request", "archive_request")):
            with self.subTest(index=index):
                self.log.unlink(missing_ok=True)
                self.config.update(fail_index=index, stderr="gh: Not Found (HTTP 404)\n")
                self.assert_failed(self.invoke(), index + 1, code, summary="gh=not_found; http=404")

    def test_allowlisted_api_status_and_error_categories(self):
        cases = (
            ("gh: Bad Request (HTTP 400)\n", "gh=bad_request; http=400"),
            ("gh: Bad credentials (HTTP 401)\n", "gh=authentication; http=401"),
            ("gh: Resource not accessible by integration (HTTP 403)\n", "gh=permission_denied; http=403"),
            ("gh: API rate limit exceeded for 192.0.2.1. (HTTP 403)\n", "gh=rate_limit; http=403"),
            ("gh: Request Timeout (HTTP 408)\n", "gh=timeout; http=408"),
            ("gh: Conflict (HTTP 409)\n", "gh=conflict; http=409"),
            ("gh: Gone (HTTP 410)\n", "gh=gone; http=410"),
            ("gh: Validation Failed (HTTP 422)\n", "gh=validation; http=422"),
            ("gh: Too Many Requests (HTTP 429)\n", "gh=rate_limit; http=429"),
            ("gh: Internal Server Error (HTTP 500)\n", "gh=server_error; http=500"),
            ("gh: Service Unavailable (HTTP 503)\n", "gh=server_error; http=503"),
            ("gh: Gateway Timeout (HTTP 504)\n", "gh=server_error; http=504"),
            ("HTTP 502: Bad Gateway (https://signed.invalid/?token=SYNTHETIC_TEST_TOKEN)\n",
             "gh=server_error; http=502"),
            ("error connecting to api.github.com\ncheck your internet connection\n", "gh=network"),
            ("To get started with GitHub CLI, please run:\n  gh auth login\n", "gh=authentication"),
        )
        for stderr, summary in cases:
            with self.subTest(summary=summary):
                self.log.unlink(missing_ok=True)
                self.config.update(fail_index=0, stderr=stderr)
                self.assert_failed(self.invoke(), 1, "artifact_request", summary=summary)

    def test_http_summary_never_echoes_secret_url_or_api_body_variants(self):
        # Mutate sensitive-looking strings rather than assuming one redaction pattern.
        for private in (PRIVATE, "SYNTHETIC_TEST_TOKEN", "ghp_abc123", "github_pat_abc123",
                        "Authorization: Bearer abc123", "token=abc123", '{"message":"abc123"}'):
            with self.subTest(private=private):
                self.log.unlink(missing_ok=True)
                self.config.update(fail_index=0,
                                   stderr=f"gh: {private} https://signed.invalid/?{private} (HTTP 404)\n")
                result = self.invoke()
                self.assert_failed(result, 1, "artifact_request", summary="gh=not_found; http=404")
                self.assertNotIn(private, result.stdout + result.stderr)

    def test_unknown_control_mutated_and_ambiguous_errors_keep_exact_fallback(self):
        for stderr in (PRIVATE + " https://signed.invalid/?secret=SYNTHETIC_TEST_TOKEN\n",
                       "gh: HTTP 404 in an arbitrary message\n", "gh: Unknown (HTTP 499)\n",
                       "gh: Not Found (HTTP 4040)\n", "gh: Not Found (HTTP 404) EXTRA\n",
                       "gh: Not Found (HTTP 4\x1b[0m04)\n", "gh: \x1b[31mNot Found (HTTP 404)\n",
                       "gh: Not\x00Found (HTTP 404)\n", "gh: Not Found (HTTP 404)\rOVERRIDE\n",
                       "gh: Not Found (HTTP 404)\ngh: Bad credentials (HTTP 401)\n"):
            with self.subTest(stderr=repr(stderr)):
                self.log.unlink(missing_ok=True)
                self.config.update(fail_index=0, stderr=stderr)
                self.assert_failed(self.invoke(), 1, "artifact_request")

    def test_oversized_stderr_is_drained_but_not_summarized_or_persisted(self):
        self.config.update(fail_index=0, stderr="gh: Not Found (HTTP 404)\n" + PRIVATE * 1024,
                           stderr_repeat=128)
        self.assert_failed(self.invoke(), 1, "artifact_request")
        self.assertFalse(list(self.root.glob("**/*stderr*")))

    def test_stderr_summary_respects_exact_capture_budget(self):
        diagnostic = "gh: Not Found (HTTP 404)\n"
        for size in (fetch_artifact.MAX_GH_STDERR_BYTES - 1,
                     fetch_artifact.MAX_GH_STDERR_BYTES, fetch_artifact.MAX_GH_STDERR_BYTES + 1):
            with self.subTest(size=size):
                self.log.unlink(missing_ok=True)
                self.config.update(fail_index=0,
                                   stderr="x" * (size - len(diagnostic) - 1) + "\n" + diagnostic)
                summary = "gh=not_found; http=404" if size <= fetch_artifact.MAX_GH_STDERR_BYTES else None
                self.assert_failed(self.invoke(), 1, "artifact_request", summary=summary)

    def test_successful_requests_never_publish_stderr_even_when_large(self):
        for repeat in (1, 128):
            with self.subTest(repeat=repeat):
                self.log.unlink(missing_ok=True)
                self.output = self.root / f"result-{repeat}"
                self.config.update(stderr="gh: Not Found (HTTP 404)\n" + PRIVATE * 128,
                                   stderr_repeat=repeat)
                result = self.invoke()
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, "")
                self.assert_calls(4)
                for name in ("selection.json", "verified.json"):
                    self.assertNotIn("gh=", (self.output / name).read_text())
                    self.assertNotIn(PRIVATE, (self.output / name).read_text())

    def test_stderr_or_process_wait_never_extends_the_existing_request_deadline(self):
        self.assertEqual(fetch_artifact.GH_TIMEOUT_SECONDS, 180)
        for close_fds, stream_stderr in (([], False), ([1], False), ([1, 2], False), ([], True)):
            with self.subTest(close_fds=close_fds, stream_stderr=stream_stderr):
                self.log.unlink(missing_ok=True)
                self.config.update(sleep_index=0, close_fds=close_fds, stream_stderr=stream_stderr,
                                   stderr="gh: Not Found (HTTP 404)\n")
                self.prepare_fixtures()
                stdout, stderr = io.StringIO(), io.StringIO()
                started = time.monotonic()
                with mock.patch.dict(os.environ, self.env), \
                        mock.patch.object(fetch_artifact, "GH_TIMEOUT_SECONDS", 0.3), \
                        mock.patch.object(sys, "argv", ["fetch_artifact.py", "--artifact-id", "501",
                                                       "--harness-sha", HARNESS,
                                                       "--output", str(self.output)]), \
                        redirect_stdout(stdout), redirect_stderr(stderr):
                    self.assertEqual(fetch_artifact.main(), 1)
                self.assertLess(time.monotonic() - started, 2)
                self.assertEqual(stdout.getvalue(), "")
                self.assertEqual(stderr.getvalue(), "Package artifact acquisition failed [artifact_request]; "
                                 "no package execution was attempted.\n")
                self.assert_calls(1)
                self.assertFalse(self.output.exists())
                self.assertEqual(list(self.root.glob(".grok-artifact-*")), [])

    def test_invalid_json_is_not_echoed_or_followed_by_another_call(self):
        (self.fixture_root / "invalid.json").write_text(PRIVATE)
        self.config["files"][0] = "invalid.json"
        self.assert_failed(self.invoke(), 1, "artifact_metadata")

    def test_deeply_nested_json_is_a_safe_failure(self):
        (self.fixture_root / "invalid.json").write_text('{"private":' + '[' * 20000 +
                                                       '"' + PRIVATE + '"' + ']' * 20000 + '}')
        self.config["files"][0] = "invalid.json"
        self.assert_failed(self.invoke(), 1, "artifact_metadata")

    def test_missing_gh_is_a_safe_failure_without_setup_or_fallback(self):
        self.env["PATH"] = str(self.root / "no-tools")
        self.assert_failed(self.invoke(), 0, "artifact_request")

    def test_mutated_download_is_rejected_without_publishing_package(self):
        archive = self.fixture_root / "artifact.zip"
        data = bytearray(archive.read_bytes())
        data[-1] ^= 1
        archive.write_bytes(data)
        self.assert_failed(self.invoke(), 4, "archive_verification")

    def test_archive_over_advertised_size_is_bounded_and_rejected(self):
        self.artifact["size_in_bytes"] = 1
        self.assert_failed(self.invoke(), 4, "archive_request")

    def test_mutated_runtime_cannot_publish_or_execute(self):
        contents = dict(self.contents)
        contents["bin/grok-bin"] += b"modified"
        self.bundle(contents)
        self.assert_failed(self.invoke(), 4, "archive_verification")

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
                self.artifact["digest"] = "sha256:" + hashlib.sha256(data).hexdigest()
                self.log.unlink(missing_ok=True)
                self.assert_failed(self.invoke(), 4, "archive_verification")

    def test_existing_destination_is_preserved_without_any_external_calls(self):
        self.output.mkdir()
        sentinel = self.output / "sentinel"
        sentinel.write_text("preserve me")
        self.assert_failed(self.invoke(), 0, "input", output_absent=False)
        self.assertEqual(list(self.output.iterdir()), [sentinel])
        self.assertEqual(sentinel.read_text(), "preserve me")

    def test_destination_created_during_download_is_preserved(self):
        self.config.update(create_output_index=3, output=str(self.output))
        self.assert_failed(self.invoke(), 4, "publication", output_absent=False)
        self.assertEqual([path.name for path in self.output.iterdir()], ["sentinel"])
        self.assertEqual((self.output / "sentinel").read_text(), "preserve me")

    def test_broken_destination_symlink_is_preserved_without_external_calls(self):
        self.output.symlink_to(self.root / "absent")
        self.assert_failed(self.invoke(), 0, "input")
        self.assertTrue(self.output.is_symlink())

    def test_invalid_inventory_json_identifies_the_parse_boundary(self):
        (self.fixture_root / "invalid.json").write_text(PRIVATE)
        for index, code in ((0, "artifact_metadata"), (1, "run_metadata"), (2, "jobs_metadata")):
            with self.subTest(code=code):
                self.log.unlink(missing_ok=True)
                original = self.config["files"][index]
                self.config["files"][index] = "invalid.json"
                self.assert_failed(self.invoke(), index + 1, code)
                self.config["files"][index] = original

    def test_missing_output_parent_identifies_private_workspace_failure(self):
        self.output = self.root / "absent" / "result"
        self.assert_failed(self.invoke(), 0, "workspace")

    def test_invalid_harness_identifies_input_failure_without_echo(self):
        self.assert_failed(self.invoke(harness=PRIVATE), 0, "input")

    def test_oversized_selected_package_is_rejected_before_download(self):
        self.artifact["size_in_bytes"] = fetch_artifact.MAX_PACKAGE_BYTES + 1
        self.assert_failed(self.invoke(), 3, "selection")


class FailureCodeTests(unittest.TestCase):
    def invoke_error(self, error):
        stdout, stderr = io.StringIO(), io.StringIO()
        with mock.patch.object(sys, "argv", ["fetch_artifact.py", "--artifact-id", "501",
                                            "--harness-sha", SHA, "--output", "unused"]), \
                mock.patch.object(fetch_artifact, "fetch", side_effect=error), \
                redirect_stdout(stdout), redirect_stderr(stderr):
            self.assertEqual(fetch_artifact.main(), 1)
        self.assertEqual(stdout.getvalue(), "")
        return stderr.getvalue()

    def test_each_stage_emits_only_its_allowlisted_code_for_private_exceptions(self):
        private = PRIVATE + " https://signed.invalid/?secret=SYNTHETIC_TEST_TOKEN"
        for code in fetch_artifact.FAILURE_CODES:
            for error_type in fetch_artifact.ACQUISITION_ERRORS:
                with self.subTest(code=code, error_type=error_type):
                    with self.assertRaises(fetch_artifact.AcquisitionFailure) as caught:
                        with fetch_artifact.acquisition_stage(code):
                            raise error_type(private)
                    self.assertEqual(self.invoke_error(caught.exception),
                                     f"Package artifact acquisition failed [{code}]; "
                                     "no package execution was attempted.\n")

    def test_unclassified_or_mutated_codes_never_become_output(self):
        mutated = fetch_artifact.AcquisitionFailure("selection")
        mutated.code = PRIVATE
        for error in (ValueError(PRIVATE), fetch_artifact.AcquisitionFailure(PRIVATE), mutated):
            with self.subTest(error_type=type(error)):
                self.assertEqual(self.invoke_error(error),
                                 "Package artifact acquisition failed [unknown]; "
                                 "no package execution was attempted.\n")

    def test_unclassified_or_mutated_gh_summaries_never_become_output(self):
        for summary in (PRIVATE, "gh=not_found; http=404\n" + PRIVATE,
                        "gh=not_found; http=499", [PRIVATE], None):
            with self.subTest(summary=repr(summary)):
                error = fetch_artifact.AcquisitionFailure("run_request", summary)
                error.gh_summary = summary
                self.assertEqual(self.invoke_error(error),
                                 "Package artifact acquisition failed [run_request]; "
                                 "no package execution was attempted.\n")

    def test_selection_reasons_preserve_only_allowlisted_codes_through_outer_boundary(self):
        for reason in ("run_state", "jobs_inventory", "identity", "producer", "artifact"):
            with self.subTest(reason=reason):
                with self.assertRaises(fetch_artifact.AcquisitionFailure) as caught:
                    with fetch_artifact.acquisition_stage("workspace"):
                        with fetch_artifact.acquisition_stage("selection"):
                            raise select_artifact.SelectionFailure(
                                reason, PRIVATE + " https://signed.invalid/?secret=SYNTHETIC_TEST_TOKEN")
                self.assertEqual(self.invoke_error(caught.exception),
                                 f"Package artifact acquisition failed [selection]; selection={reason}; "
                                 "no package execution was attempted.\n")

    def test_unclassified_or_mutated_selection_reasons_never_become_output(self):
        class StringSubclass(str):
            pass
        for reason in (PRIVATE, "run_state\n" + PRIVATE, "https://signed.invalid/",
                       [PRIVATE], None, StringSubclass("run_state")):
            with self.subTest(reason=repr(reason)):
                error = fetch_artifact.AcquisitionFailure("selection", selection_reason=reason)
                error.selection_reason = reason
                self.assertEqual(self.invoke_error(error),
                                 "Package artifact acquisition failed [selection]; "
                                 "no package execution was attempted.\n")
                selector_error = select_artifact.SelectionFailure("run_state", PRIVATE)
                selector_error.reason = reason
                with self.assertRaises(fetch_artifact.AcquisitionFailure) as caught:
                    with fetch_artifact.acquisition_stage("selection"):
                        raise selector_error
                self.assertEqual(self.invoke_error(caught.exception),
                                 "Package artifact acquisition failed [selection]; "
                                 "no package execution was attempted.\n")

    def test_selection_reason_cannot_cross_another_stage_or_mask_unknown_failure(self):
        for code in ("input", "run_request", "unknown", PRIVATE):
            with self.subTest(code=code):
                error = fetch_artifact.AcquisitionFailure(code, selection_reason="run_state")
                error.selection_reason = "run_state"
                safe_code = code if code in fetch_artifact.FAILURE_CODES else "unknown"
                self.assertEqual(self.invoke_error(error),
                                 f"Package artifact acquisition failed [{safe_code}]; "
                                 "no package execution was attempted.\n")
        plain_error = ValueError(PRIVATE)
        plain_error.reason = "run_state"
        with self.assertRaises(fetch_artifact.AcquisitionFailure) as caught:
            with fetch_artifact.acquisition_stage("selection"):
                raise plain_error
        self.assertEqual(self.invoke_error(caught.exception),
                         "Package artifact acquisition failed [selection]; "
                         "no package execution was attempted.\n")

    def test_outer_workspace_boundary_preserves_the_inner_failure(self):
        with self.assertRaises(fetch_artifact.AcquisitionFailure) as caught:
            with fetch_artifact.acquisition_stage("workspace"):
                with fetch_artifact.acquisition_stage("archive_request"):
                    raise TimeoutError(PRIVATE)
        self.assertEqual(caught.exception.code, "archive_request")


class PRWorkflowTests(unittest.TestCase):
    def test_source_admission_is_separate_from_catalog_selection(self):
        workflow = Path(__file__).resolve().parents[2] / ".github/workflows/grok-live.yml"
        text = workflow.read_text().split("\njobs:\n", 1)[1]
        self.assertIn("github.repository == 'Harness-X-Harness/codex'", text)
        self.assertIn("github.ref_name == inputs.formal_line", text)
        self.assertNotIn("image-primary", text)
        self.assertLess(text.index("Validate the explicit Go-owned selection before backend use"),
                        text.index("GROK_API_KEY:"))
        self.assertLess(text.index("Fetch and verify the explicit package"),
                        text.index("GROK_API_KEY:"))
        self.assertEqual(text.count("GROK_API_KEY:"), 2)
        finalizer = text.split("      - name: Preserve bounded runner errors after execution\n", 1)[1].split("      - ", 1)[0]
        self.assertIn("always() && contains(fromJSON('[\"success\",\"failure\",\"cancelled\"]'), steps.live_execution.outcome)", finalizer)
        self.assertIn("live_diagnostics.py --stderr", finalizer)
        self.assertNotIn("go test", finalizer)
        def script(name):
            step = text.split("      - name: " + name + "\n", 1)[1].split("      - ", 1)[0]
            return textwrap.dedent(step.split("        run: |\n", 1)[1])
        bind = script("Bind the harness subject")
        fetch = script("Fetch and verify the explicit package")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "git").write_text("#!/bin/sh\nif [ \"$1\" = rev-parse ]; then printf '%s\\n' \"$GITHUB_SHA\"; fi\n")
            (root / "python3").write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CAPTURE\"\n")
            for name in ("git", "python3"):
                (root / name).chmod(0o755)
            for branch, line in (("fix/task", ""), ("grok/rust-v0.164.0", ""),
                                 ("grok/rust-v0.164.0", "grok/rust-v0.164.0")):
                with self.subTest(branch=branch, line=line):
                    capture = root / "capture"
                    capture.unlink(missing_ok=True)
                    env = dict(os.environ, PATH=str(root) + os.pathsep + os.environ["PATH"],
                               GITHUB_REF="refs/heads/" + branch, GITHUB_REF_NAME=branch, GITHUB_SHA=HARNESS,
                               GITHUB_WORKFLOW_SHA=HARNESS, FORMAL_LINE=line,
                               BINARY_ARTIFACT_ID="501", RUNNER_TEMP=str(root), CAPTURE=str(capture))
                    result = subprocess.run(["bash", "-c", bind + "\n" + fetch], env=env,
                                            capture_output=True, text=True, timeout=5)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertTrue(capture.exists())
                    self.assertEqual("--harness-ref" in capture.read_text().splitlines(),
                                     not branch.startswith("grok/rust-v"))
                    self.assertEqual("--expected-line" in capture.read_text().splitlines(), bool(line))


if __name__ == "__main__":
    unittest.main()

