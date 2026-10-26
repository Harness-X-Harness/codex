import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import select_artifact as selection
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


def pr_fixture():
    run, jobs, artifacts = fixture()
    repo = {"id": 123, "full_name": REPOSITORY}
    pr = {"number": 444, "state": "open", "merged": False, "merge_commit_sha": SHA,
          "head": {"sha": HARNESS, "ref": "fix/image", "repo": dict(repo)},
          "base": {"sha": "d" * 40, "ref": "grok/rust-v0.164.0", "repo": dict(repo)}}
    run.update(event="pull_request", head_sha=HARNESS, head_branch="fix/image",
               pull_requests=[copy.deepcopy(pr)])
    artifacts["artifacts"][0]["workflow_run"]["head_sha"] = HARNESS
    return run, jobs, artifacts, pr


class ArtifactTests(unittest.TestCase):
    def test_pr_diagnostic_binds_head_and_distinct_tested_merge_source(self):
        run, jobs, artifacts, pr = pr_fixture()
        got = select(run, jobs, artifacts, 37, HARNESS, pull_request=pr)
        self.assertEqual(got["source_sha"], SHA)
        self.assertEqual(got["pr_head_sha"], HARNESS)
        self.assertEqual(got["harness_sha"], HARNESS)
        self.assertEqual(got["pull_request_number"], 444)
        self.assertEqual(got["selection_mode"], "pr_diagnostic")
        self.assertEqual(got["artifact_name"], NAME)
        with self.assertRaises(ValueError):
            select(run, jobs, artifacts, 37, HARNESS)
        with self.assertRaises(ValueError):
            select(run, jobs, artifacts, 37, HARNESS, pull_request=pr,
                   same_run_source_sha=HARNESS, expected_artifact_id=501, expected_run_attempt=2)

    def test_pr_diagnostic_rejects_stale_fork_unlinked_or_wrong_subject(self):
        for mode in ("fork", "base_fork", "wrong_number", "closed", "merged", "stale_head",
                     "stale_base", "stale_base_ref", "stale_merge", "missing_merge", "wrong_harness",
                     "wrong_ref", "not_version_line", "no_link", "two_links", "wrong_link",
                     "wrong_link_repo", "failed_ci", "wrong_run_repo", "wrong_workflow",
                     "artifact_head_is_merge", "expired", "failed_build"):
            with self.subTest(mode=mode):
                run, jobs, artifacts, pr = pr_fixture()
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
                elif mode == "artifact_head_is_merge": artifacts["artifacts"][0]["workflow_run"]["head_sha"] = SHA
                elif mode == "expired": artifacts["artifacts"][0]["expired"] = True
                elif mode == "failed_build": jobs["jobs"][0]["conclusion"] = "failure"
                with self.assertRaises(ValueError):
                    select(run, jobs, artifacts, 37, harness, pull_request=pr)

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

    def same_run_select(self, run, jobs, artifacts):
        return select(run, jobs, artifacts, 37, SHA, same_run_source_sha=SHA,
                      expected_artifact_id=501, expected_run_attempt=2)

    def assert_selection_failure(self, reason, operation):
        with self.assertRaises(ValueError) as raised:
            operation()
        self.assertEqual(getattr(raised.exception, "reason", None), reason)
        self.assertIsInstance(raised.exception, selection.SelectionFailure)

    def test_same_run_accepts_only_queued_or_in_progress_with_valid_producer(self):
        for status in ("queued", "in_progress"):
            with self.subTest(status=status):
                run, jobs, artifacts = fixture()
                run.update(status=status, conclusion=None)
                # Queued is a run snapshot, not permission to use a queued build.
                jobs["jobs"].append({"name": "Grok Live (sibling)", "status": "queued",
                                     "conclusion": None})
                jobs["total_count"] = 2
                got = self.same_run_select(run, jobs, artifacts)
                self.assertEqual(got["producer_job_id"], 101)
                self.assertEqual(got["artifact_id"], 501)
                self.assertEqual(got["run_attempt"], 2)
                self.assertEqual(got["source_sha"], got["harness_sha"])
                self.assertEqual(got["selection_mode"], "same_run")
                self.assertEqual(got["run_status"], status)
                self.assertIsNone(got["run_conclusion"])

    def test_same_run_rejects_every_other_run_status(self):
        for status in ("completed", "waiting", "pending", "requested", "action_required",
                       "cancelled", "failure", "neutral", "skipped", "stale", "startup_failure",
                       "success", "timed_out", "PRIVATE_CANARY", "", None, 0):
            with self.subTest(status=status):
                run, jobs, artifacts = fixture()
                run.update(status=status, conclusion=None)
                self.assert_selection_failure("run_state", lambda: self.same_run_select(run, jobs, artifacts))

    def test_same_run_rejects_every_non_null_conclusion(self):
        for status in ("queued", "in_progress"):
            for conclusion in ("success", "failure", "cancelled", "timed_out", "action_required",
                               "neutral", "skipped", "stale", "startup_failure", "", False, 0,
                               "PRIVATE_CANARY"):
                with self.subTest(status=status, conclusion=conclusion):
                    run, jobs, artifacts = fixture()
                    run.update(status=status, conclusion=conclusion)
                    self.assert_selection_failure("run_state", lambda: self.same_run_select(run, jobs, artifacts))

    def test_same_run_preserves_every_run_identity_check(self):
        mutations = [
            ("id", 38), ("run_attempt", 1), ("event", "pull_request"),
            ("path", ".github/workflows/other.yml"), ("head_branch", "main"), ("head_sha", "a"),
            ("repository", {"full_name": "other/repo"}), ("head_repository", {"full_name": "fork/repo"}),
        ]
        for status in ("queued", "in_progress"):
            for key, value in mutations:
                with self.subTest(status=status, field=key):
                    run, jobs, artifacts = fixture()
                    run.update(status=status, conclusion=None)
                    run[key] = value
                    self.assert_selection_failure("identity", lambda: self.same_run_select(run, jobs, artifacts))

    def test_same_run_preserves_exact_successful_unique_producer(self):
        for status in ("queued", "in_progress"):
            for mode in ("missing", "two_successes", "success_and_unfinished", "failed",
                         "queued", "in_progress", "old_attempt", "wrong_run", "invalid_id"):
                with self.subTest(status=status, mode=mode):
                    run, jobs, artifacts = fixture()
                    run.update(status=status, conclusion=None)
                    if mode == "missing":
                        jobs["jobs"] = []
                    elif mode in ("two_successes", "success_and_unfinished"):
                        duplicate = copy.deepcopy(jobs["jobs"][0])
                        duplicate["id"] = 102
                        if mode == "success_and_unfinished":
                            duplicate.update(status="in_progress", conclusion=None)
                        jobs["jobs"].append(duplicate)
                    elif mode == "failed":
                        jobs["jobs"][0]["conclusion"] = "failure"
                    elif mode in ("queued", "in_progress"):
                        jobs["jobs"][0].update(status=mode, conclusion=None)
                    elif mode == "old_attempt":
                        jobs["jobs"][0]["run_attempt"] = 1
                    elif mode == "wrong_run":
                        jobs["jobs"][0]["run_id"] = 38
                    else:
                        jobs["jobs"][0]["id"] = 0
                    # Keep counts equal so this proves the producer check itself.
                    jobs["total_count"] = len(jobs["jobs"])
                    self.assert_selection_failure("producer", lambda: self.same_run_select(run, jobs, artifacts))

    def test_same_run_preserves_artifact_identity_and_availability(self):
        for status in ("queued", "in_progress"):
            for mode in ("missing", "ambiguous", "expired", "wrong_id", "wrong_source",
                         "wrong_run", "old_attempt", "digest", "empty"):
                with self.subTest(status=status, mode=mode):
                    run, jobs, artifacts = fixture()
                    run.update(status=status, conclusion=None)
                    item = artifacts["artifacts"][0]
                    if mode == "missing":
                        artifacts["artifacts"] = []
                    elif mode == "ambiguous":
                        artifacts["artifacts"].append(copy.deepcopy(item))
                    elif mode == "expired":
                        item["expired"] = True
                    elif mode == "wrong_id":
                        item["id"] = 502
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
                    artifacts["total_count"] = len(artifacts["artifacts"])
                    self.assert_selection_failure("artifact", lambda: self.same_run_select(run, jobs, artifacts))

    def test_job_and_artifact_inventory_counts_stay_strict_in_all_modes(self):
        for status in ("queued", "in_progress", "completed"):
            for inventory in ("jobs", "artifacts"):
                for count in (0, 2, -1, 101, "1", None):
                    with self.subTest(status=status, inventory=inventory, total_count=count):
                        run, jobs, artifacts = fixture()
                        if status != "completed":
                            run.update(status=status, conclusion=None)
                        (jobs if inventory == "jobs" else artifacts)["total_count"] = count
                        operation = (lambda: select(run, jobs, artifacts, 37, HARNESS)) if status == "completed" else (
                            lambda: self.same_run_select(run, jobs, artifacts))
                        self.assert_selection_failure(inventory + "_inventory", operation)

    def test_job_and_artifact_inventories_must_be_lists(self):
        for status in ("queued", "in_progress", "completed"):
            for inventory in ("jobs", "artifacts"):
                for rows in (None, {}, "PRIVATE_CANARY"):
                    with self.subTest(status=status, inventory=inventory, rows=rows):
                        run, jobs, artifacts = fixture()
                        if status != "completed":
                            run.update(status=status, conclusion=None)
                        (jobs if inventory == "jobs" else artifacts)[inventory] = rows
                        operation = (lambda: select(run, jobs, artifacts, 37, HARNESS)) if status == "completed" else (
                            lambda: self.same_run_select(run, jobs, artifacts))
                        self.assert_selection_failure(inventory + "_inventory", operation)

    def test_diagnostic_mode_keeps_its_settled_run_requirement(self):
        for conclusion in ("success", "failure", "cancelled", "timed_out", "action_required",
                           "neutral", "skipped", "stale", "startup_failure"):
            with self.subTest(conclusion=conclusion):
                run, jobs, artifacts = fixture()
                run["conclusion"] = conclusion
                self.assertEqual(select(run, jobs, artifacts, 37, HARNESS)["run_conclusion"], conclusion)
        for status, conclusion in (("queued", None), ("in_progress", None), ("completed", None),
                                   ("completed", "PRIVATE_CANARY")):
            with self.subTest(status=status, conclusion=conclusion):
                run, jobs, artifacts = fixture()
                run.update(status=status, conclusion=conclusion)
                self.assert_selection_failure("run_state", lambda: select(run, jobs, artifacts, 37, HARNESS))

    def test_selection_failure_reason_is_a_fixed_allowlist(self):
        self.assertEqual(selection.SELECTION_FAILURE_REASONS, frozenset({
            "run_state", "jobs_inventory", "artifacts_inventory", "identity", "producer", "artifact",
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

