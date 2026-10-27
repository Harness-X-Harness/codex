import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import select_artifact as selection
from select_artifact import REPOSITORY, TARGET, select, admit_harness


SHA = "a" * 40
HARNESS = "b" * 40
NAME = f"grok-{SHA}-{TARGET}-37-2"


def fixture():
    run = {"id": 37, "run_attempt": 2, "head_sha": SHA, "status": "completed",
           "conclusion": "success", "event": "push", "path": ".github/workflows/grok.yml",
           "head_branch": "grok/rust-v0.158.0", "repository": {"full_name": REPOSITORY},
           "head_repository": {"full_name": REPOSITORY}}
    jobs = {"total_count": 1, "jobs": [{"id": 101, "run_id": 37, "run_attempt": 2,
             "name": "Build " + TARGET, "status": "completed", "conclusion": "success"}]}
    artifact = {"id": 501, "name": NAME, "expired": False,
                "digest": "sha256:" + "c" * 64, "size_in_bytes": 4096,
                "workflow_run": {"id": 37, "head_sha": SHA}}
    return run, jobs, artifact


def pr_fixture():
    run, jobs, artifact = fixture()
    repo = {"id": 123, "full_name": REPOSITORY}
    pr = {"number": 444, "state": "open", "merged": False, "merge_commit_sha": SHA,
          "head": {"sha": HARNESS, "ref": "fix/image", "repo": dict(repo)},
          "base": {"sha": "d" * 40, "ref": "grok/rust-v0.164.0", "repo": dict(repo)}}
    run.update(event="pull_request", head_sha=HARNESS, head_branch="fix/image",
               pull_requests=[copy.deepcopy(pr)])
    artifact["workflow_run"]["head_sha"] = HARNESS
    return run, jobs, artifact, pr


class ArtifactTests(unittest.TestCase):
    def test_pr_diagnostic_binds_head_and_distinct_tested_merge_source(self):
        run, jobs, artifact, pr = pr_fixture()
        self.assertEqual(admit_harness(pr, run, HARNESS, "fix/image"), SHA)
        got = select(run, jobs, artifact, 501, HARNESS, pull_request=pr)
        self.assertEqual(got["source_sha"], SHA)
        self.assertEqual(got["pr_head_sha"], HARNESS)
        self.assertEqual(got["harness_sha"], HARNESS)
        self.assertEqual(got["pull_request_number"], 444)
        self.assertEqual(got["selection_mode"], "pr_diagnostic")
        self.assertEqual(got["artifact_name"], NAME)
        with self.assertRaises(ValueError):
            select(run, jobs, artifact, 501, HARNESS)
        with self.assertRaises(ValueError):
            select(run, jobs, artifact, 501, HARNESS, pull_request=pr,
                   expected_line="grok/rust-v0.164.0")

    def test_pr_diagnostic_rejects_stale_fork_unlinked_or_wrong_subject(self):
        for mode in ("fork", "base_fork", "wrong_number", "closed", "merged", "stale_head",
                     "stale_base", "stale_base_ref", "stale_merge", "missing_merge", "wrong_harness",
                     "wrong_ref", "not_version_line", "no_link", "two_links", "wrong_link",
                     "wrong_link_repo", "failed_ci", "wrong_run_repo", "wrong_workflow",
                     "artifact_head_is_merge", "expired", "failed_build"):
            with self.subTest(mode=mode):
                run, jobs, artifact, pr = pr_fixture()
                harness = HARNESS
                if mode == "fork": pr["head"]["repo"]["full_name"] = "fork/codex"
                elif mode == "base_fork": pr["base"]["repo"]["full_name"] = "other/codex"
                elif mode == "wrong_number": pr["number"] = 445
                elif mode == "closed": pr["state"] = "closed"
                elif mode == "merged": pr["merged"] = True
                elif mode == "stale_head": pr["head"]["sha"] = "e" * 40
                elif mode == "stale_base": pr["base"]["sha"] = "e" * 40
                elif mode == "stale_base_ref": pr["base"]["ref"] = "grok/rust-v0.165.0"
                elif mode == "stale_merge": pr["merge_commit_sha"] = "e" * 40
                elif mode == "missing_merge": pr["merge_commit_sha"] = None
                elif mode == "wrong_harness": harness = "e" * 40
                elif mode == "wrong_ref": run["head_branch"] = "unrelated"
                elif mode == "not_version_line": pr["base"]["ref"] = "main"
                elif mode == "no_link": run["pull_requests"] = []
                elif mode == "two_links": run["pull_requests"] *= 2
                elif mode == "wrong_link": run["pull_requests"][0]["head"]["sha"] = "e" * 40
                elif mode == "wrong_link_repo": run["pull_requests"][0]["head"]["repo"]["id"] = 456
                elif mode == "failed_ci": run["conclusion"] = "failure"
                elif mode == "wrong_run_repo": run["head_repository"]["full_name"] = "fork/codex"
                elif mode == "wrong_workflow": run["path"] = "other.yml"
                elif mode == "artifact_head_is_merge": artifact["workflow_run"]["head_sha"] = SHA
                elif mode == "expired": artifact["expired"] = True
                elif mode == "failed_build": jobs["jobs"][0]["conclusion"] = "failure"
                with self.assertRaises(ValueError):
                    admit_harness(pr, run, harness, "fix/image")
                    select(run, jobs, artifact, 501, harness, pull_request=pr)

    def test_exact_attempt_identity_and_sibling_failure_are_retained(self):
        run, jobs, artifact = fixture()
        run["conclusion"] = "failure"
        jobs["jobs"].append({"name": "Build aarch64-apple-darwin", "conclusion": "failure"})
        jobs["total_count"] = 2
        self.assertEqual(select(run, jobs, artifact, 501, HARNESS), {
            "repository": REPOSITORY, "run_id": 37, "run_attempt": 2, "source_sha": SHA,
            "target": TARGET, "harness_sha": HARNESS, "artifact_id": 501, "artifact_name": NAME,
            "artifact_digest": "sha256:" + "c" * 64, "artifact_size": 4096,
            "producer_job_id": 101, "run_conclusion": "failure",
            "selection_mode": "diagnostic", "run_status": "completed",
        })

    def test_rejects_wrong_repository_source_and_workflow(self):
        mutations = [
            ("id", 38), ("event", "pull_request"),
            ("path", ".github/workflows/other.yml"), ("head_branch", "main"), ("head_sha", "a"),
            ("repository", {"full_name": "other/repo"}), ("head_repository", {"full_name": "fork/repo"}),
        ]
        for key, value in mutations:
            with self.subTest(field=key):
                run, jobs, artifact = fixture()
                run[key] = value
                with self.assertRaises(ValueError):
                    select(run, jobs, artifact, 501, HARNESS)

    def test_rejects_missing_ambiguous_incomplete_or_failed_producer(self):
        for mode in ("missing", "ambiguous", "incomplete", "failed", "old_attempt", "wrong_run"):
            with self.subTest(mode=mode):
                run, jobs, artifact = fixture()
                if mode == "missing":
                    jobs["jobs"] = []
                    jobs["total_count"] = 0
                elif mode == "ambiguous":
                    jobs["jobs"] *= 2
                    jobs["total_count"] = 2
                elif mode == "incomplete":
                    jobs["total_count"] += 1
                elif mode == "failed":
                    jobs["jobs"][0]["conclusion"] = "failure"
                elif mode == "old_attempt":
                    jobs["jobs"][0]["run_attempt"] = 1
                else:
                    jobs["jobs"][0]["run_id"] = 38
                with self.assertRaises(ValueError):
                    select(run, jobs, artifact, 501, HARNESS)

    def test_rejects_unavailable_or_misidentified_artifact(self):
        for key, value in (("id", 502), ("expired", True), ("digest", ""),
                           ("size_in_bytes", 0), ("name", NAME[:-1] + "1"),
                           ("workflow_run", {"id": 37, "head_sha": "d" * 40})):
            with self.subTest(field=key):
                run, jobs, artifact = fixture()
                artifact[key] = value
                with self.assertRaises(ValueError):
                    select(run, jobs, artifact, 501, HARNESS)

    def test_original_artifact_survives_later_attempt_and_running_sibling(self):
        run, jobs, artifact = fixture()
        run.update(run_attempt=3, status="in_progress", conclusion=None)
        jobs["jobs"].append({"name": "Build aarch64-apple-darwin", "status": "in_progress"})
        jobs["total_count"] = 2
        got = select(run, jobs, artifact, 501, SHA, expected_line="grok/rust-v0.158.0")
        self.assertEqual(got["run_attempt"], 2)
        self.assertEqual(got["artifact_id"], 501)
        self.assertEqual(got["selection_mode"], "line_bound")
        jobs["jobs"][0]["name"] = "Build " + TARGET + " / Package"
        self.assertEqual(select(run, jobs, artifact, 501, SHA,
                                expected_line="grok/rust-v0.158.0")["producer_job_id"], 101)
        self.assertEqual(got["run_status"], "in_progress")
        self.assertIsNone(got["run_conclusion"])
        run.update(status="PRIVATE_CANARY", conclusion="PRIVATE_CANARY")
        observed = select(run, jobs, artifact, 501, SHA, expected_line="grok/rust-v0.158.0")
        self.assertEqual(observed["run_status"], "unknown")
        self.assertEqual(observed["run_conclusion"], "unknown")
        with self.assertRaises(ValueError):
            select(run, jobs, artifact, 501, HARNESS, expected_line="grok/rust-v0.164.0")

    def test_selection_failure_reason_is_a_fixed_allowlist(self):
        self.assertEqual(selection.SELECTION_FAILURE_REASONS, frozenset({
            "run_state", "jobs_inventory", "identity", "producer", "artifact",
        }))
        for reason in selection.SELECTION_FAILURE_REASONS:
            failure = selection.SelectionFailure(reason, "safe message")
            self.assertIsInstance(failure, ValueError)
            self.assertEqual(failure.reason, reason)
            self.assertEqual(str(failure), "safe message")
        with self.assertRaises(ValueError) as raised:
            selection.SelectionFailure("PRIVATE_CANARY", "safe message")
        self.assertNotIn("PRIVATE_CANARY", str(raised.exception))

    def test_real_command_emits_only_selected_safe_metadata_or_nonzero(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            args = [sys.executable, str(Path(__file__).with_name("select_artifact.py")),
                    "--artifact-id", "501", "--harness-sha", HARNESS]
            for label, body in zip(("run", "jobs", "artifact"), fixture()):
                body["private_marker"] = "PRIVATE_CANARY"
                path = root / (label + ".json")
                path.write_text(json.dumps(body))
                args += ["--" + label + "-json", str(path)]
            result = subprocess.run(args, capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout), select(*fixture(), 501, HARNESS))
            self.assertNotIn("PRIVATE_CANARY", result.stdout + result.stderr)
            (root / "artifact.json").write_text('{"invalid": "PRIVATE_CANARY"}')
            failed = subprocess.run(args, capture_output=True, text=True, timeout=5)
            self.assertNotEqual(failed.returncode, 0)
            self.assertEqual(failed.stdout, "")
            self.assertNotIn("PRIVATE_CANARY", failed.stderr)


if __name__ == "__main__":
    unittest.main()

