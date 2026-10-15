import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from select_artifact import REPOSITORY, TARGET, select


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
    artifacts = {"total_count": 1, "artifacts": [{"id": 501, "name": NAME, "expired": False,
                  "digest": "sha256:" + "c" * 64, "size_in_bytes": 4096,
                  "workflow_run": {"id": 37, "head_sha": SHA}}]}
    return run, jobs, artifacts


class ArtifactTests(unittest.TestCase):
    def test_exact_attempt_identity_and_sibling_failure_are_retained(self):
        run, jobs, artifacts = fixture()
        run["conclusion"] = "failure"
        jobs["jobs"].append({"name": "Build aarch64-apple-darwin", "conclusion": "failure"})
        jobs["total_count"] = 2
        self.assertEqual(select(run, jobs, artifacts, 37, HARNESS), {
            "repository": REPOSITORY, "run_id": 37, "run_attempt": 2, "source_sha": SHA,
            "target": TARGET, "harness_sha": HARNESS, "artifact_id": 501, "artifact_name": NAME,
            "artifact_digest": "sha256:" + "c" * 64, "artifact_size": 4096,
            "producer_job_id": 101, "run_conclusion": "failure",
            "selection_mode": "diagnostic", "run_status": "completed",
        })

    def test_rejects_unsettled_wrong_repository_source_and_workflow(self):
        mutations = [
            ("id", 38), ("run_attempt", 1), ("status", "in_progress"), ("event", "pull_request"),
            ("path", ".github/workflows/other.yml"), ("head_branch", "main"), ("head_sha", "a"),
            ("repository", {"full_name": "other/repo"}), ("head_repository", {"full_name": "fork/repo"}),
        ]
        for key, value in mutations:
            with self.subTest(field=key):
                run, jobs, artifacts = fixture()
                run[key] = value
                with self.assertRaises(ValueError):
                    select(run, jobs, artifacts, 37, HARNESS)

    def test_rejects_missing_ambiguous_incomplete_or_failed_producer(self):
        for mode in ("missing", "ambiguous", "incomplete", "failed", "old_attempt", "wrong_run"):
            with self.subTest(mode=mode):
                run, jobs, artifacts = fixture()
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
                    select(run, jobs, artifacts, 37, HARNESS)

    def test_rejects_unavailable_wrong_old_or_ambiguous_artifacts(self):
        for mode in ("missing", "ambiguous", "incomplete", "expired", "wrong_source", "wrong_run", "old_attempt", "digest", "empty"):
            with self.subTest(mode=mode):
                run, jobs, artifacts = fixture()
                item = artifacts["artifacts"][0]
                if mode == "missing":
                    artifacts["artifacts"] = []
                    artifacts["total_count"] = 0
                elif mode == "ambiguous":
                    artifacts["artifacts"].append(copy.deepcopy(item))
                    artifacts["total_count"] = 2
                elif mode == "incomplete":
                    artifacts["total_count"] = 2
                elif mode == "expired":
                    item["expired"] = True
                elif mode == "wrong_source":
                    item["workflow_run"]["head_sha"] = "d" * 40
                elif mode == "wrong_run":
                    item["workflow_run"]["id"] = 38
                elif mode == "old_attempt":
                    item["name"] = NAME[:-1] + "1"
                elif mode == "digest":
                    item["digest"] = ""
                else:
                    item["size_in_bytes"] = 0
                with self.assertRaises(ValueError):
                    select(run, jobs, artifacts, 37, HARNESS)

    def test_same_run_consumes_only_the_completed_exact_linux_producer(self):
        run, jobs, artifacts = fixture()
        run.update(status="in_progress", conclusion=None)
        jobs["jobs"].append({"name": "Build aarch64-apple-darwin", "status": "completed", "conclusion": "failure"})
        jobs["total_count"] = 2
        got = select(run, jobs, artifacts, 37, SHA,
                     same_run_source_sha=SHA, expected_artifact_id=501, expected_run_attempt=2)
        self.assertEqual(got["artifact_id"], 501)
        self.assertEqual(got["source_sha"], got["harness_sha"])
        self.assertEqual(got["selection_mode"], "same_run")
        self.assertEqual(got["run_status"], "in_progress")
        self.assertIsNone(got["run_conclusion"])
        self.assertEqual(jobs["jobs"][1]["conclusion"], "failure")
        with self.assertRaises(ValueError):
            select(run, jobs, artifacts, 37, SHA)

    def test_same_run_rejects_wrong_binding_and_unfinished_build_without_fallback(self):
        for mode in ("wrong_harness", "wrong_source", "wrong_artifact", "missing_artifact", "missing_source",
                     "settled", "concluded_while_running", "unfinished_build", "failed_build", "invalid_job_id", "wrong_attempt"):
            with self.subTest(mode=mode):
                run, jobs, artifacts = fixture()
                run.update(status="in_progress", conclusion=None)
                harness, source, artifact, attempt = SHA, SHA, 501, 2
                if mode == "wrong_harness": harness = HARNESS
                elif mode == "wrong_source": source = "c" * 40
                elif mode == "wrong_artifact": artifact = 502
                elif mode == "missing_artifact": artifact = None
                elif mode == "missing_source": source = None
                elif mode == "settled": run.update(status="completed", conclusion="success")
                elif mode == "concluded_while_running": run["conclusion"] = "success"
                elif mode == "unfinished_build": jobs["jobs"][0].update(status="in_progress", conclusion=None)
                elif mode == "failed_build": jobs["jobs"][0]["conclusion"] = "failure"
                elif mode == "wrong_attempt": attempt = 3
                else: jobs["jobs"][0]["id"] = 0
                with self.assertRaises(ValueError):
                    select(run, jobs, artifacts, 37, harness,
                           same_run_source_sha=source, expected_artifact_id=artifact, expected_run_attempt=attempt)

    def test_real_command_emits_only_selected_safe_metadata_or_nonzero(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            args = [sys.executable, str(Path(__file__).with_name("select_artifact.py")),
                    "--run-id", "37", "--harness-sha", HARNESS]
            for label, body in zip(("run", "jobs", "artifacts"), fixture()):
                body["private_marker"] = "PRIVATE_CANARY"
                path = root / (label + ".json")
                path.write_text(json.dumps(body))
                args += ["--" + label + "-json", str(path)]
            result = subprocess.run(args, capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout), select(*fixture(), 37, HARNESS))
            self.assertNotIn("PRIVATE_CANARY", result.stdout + result.stderr)
            (root / "artifacts.json").write_text('{"invalid": "PRIVATE_CANARY"}')
            failed = subprocess.run(args, capture_output=True, text=True, timeout=5)
            self.assertNotEqual(failed.returncode, 0)
            self.assertEqual(failed.stdout, "")
            self.assertNotIn("PRIVATE_CANARY", failed.stderr)


if __name__ == "__main__":
    unittest.main()
