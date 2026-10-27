import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

import build_selection as selection
from select_artifact import REPOSITORY, TARGET


LINE = "grok/rust-v0.164.0"
SHA = "a" * 40
HEAD = "b" * 40


def fixture():
    run = {"id": 37, "run_number": 12, "run_attempt": 9, "head_sha": SHA,
           "head_branch": LINE, "event": "push", "path": ".github/workflows/grok.yml",
           "status": "in_progress", "conclusion": None,
           "repository": {"full_name": REPOSITORY}, "head_repository": {"full_name": REPOSITORY}}
    artifact = {"id": 501, "name": f"grok-{SHA}-{TARGET}-37-2", "expired": False,
                "workflow_run": {"id": 37, "head_sha": SHA}, "digest": "sha256:" + "c" * 64,
                "size_in_bytes": 100}
    job = {"id": 101, "name": "Build " + TARGET, "run_id": 37, "run_attempt": 2,
           "status": "completed", "conclusion": "success"}
    return run, artifact, job


class BuildSelectionTests(unittest.TestCase):
    def test_coarse_consumer_boundaries_keep_unknown_and_build_inputs(self):
        for path in ("grok/live/image.go", "grok/tests/integration/child_test.go", "grok/go.mod",
                     "grok/package/fetch_artifact.py", "grok/package/test_stage.py",
                     "grok/package/verify_provider.sh", ".github/workflows/grok.yml", "grok/docs/readme.md"):
            with self.subTest(path=path):
                self.assertFalse(selection.is_binary_input(path))
        for path in ("codex-rs/core/src/lib.rs", "codex-rs/core/tests/test.rs", "grok/dist/models.json",
                     "grok/package/stage.py", ".github/workflows/grok-build.yml",
                     ".github/actions/build-grok/action.yml", "scripts/codex_package/package.py", "unknown.file"):
            with self.subTest(path=path):
                self.assertTrue(selection.is_binary_input(path))

    def test_full_git_diff_includes_earlier_change_delete_and_both_rename_sides(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            def git(*args):
                return subprocess.check_output(["git", *args], cwd=root, stderr=subprocess.DEVNULL, text=True).strip()
            git("init", "-q")
            git("config", "user.name", "fixture")
            git("config", "user.email", "fixture@example.invalid")
            (root / "codex-rs").mkdir()
            (root / "codex-rs/runtime.rs").write_text("binary")
            git("add", "."); git("commit", "-qm", "base"); base = git("rev-parse", "HEAD")
            (root / "codex-rs/runtime.rs").write_text("changed")
            git("add", "."); git("commit", "-qm", "binary change")
            (root / "README.md").write_text("docs")
            git("add", "."); git("commit", "-qm", "docs"); head = git("rev-parse", "HEAD")
            self.assertTrue(selection.ancestor(root, base, head))
            self.assertTrue(selection.binary_inputs_changed(root, base, head))
            self.assertFalse(selection.binary_inputs_changed(root, git("rev-parse", "HEAD~1"), head))
            (root / "docs").mkdir()
            (root / "codex-rs/runtime.rs").rename(root / "docs/runtime.md")
            git("add", "-A"); git("commit", "-qm", "move binary to docs"); moved = git("rev-parse", "HEAD")
            self.assertTrue(selection.binary_inputs_changed(root, head, moved))
            with self.assertRaises(subprocess.SubprocessError):
                selection.binary_inputs_changed(root, "0" * 40, moved)

    def invoke(self, run, artifact, job, *, changed=False, ancestor=True):
        endpoints = []
        def inventory(endpoint, key):
            endpoints.append(endpoint)
            return {"workflow_runs": [run], "artifacts": [artifact], "jobs": [job]}[key]
        with mock.patch.object(selection, "newest_runs", return_value=iter([run])), \
             mock.patch.object(selection, "inventory", side_effect=inventory), \
             mock.patch.object(selection, "ancestor", return_value=ancestor), \
             mock.patch.object(selection, "binary_inputs_changed", return_value=changed), \
             mock.patch.object(selection, "fetch", return_value={"artifact_id": 501, "source_sha": SHA}) as fetch:
            result = selection.plan(LINE, HEAD, Path.cwd())
            return result, endpoints, fetch.call_count

    def test_reuses_original_producer_while_parent_runs_or_after_partial_rerun(self):
        run, artifact, job = fixture()
        for status, conclusion in (("in_progress", None), ("completed", "cancelled")):
            run.update(status=status, conclusion=conclusion)
            result, endpoints, calls = self.invoke(run, artifact, job)
            self.assertFalse(result["build_needed"])
            self.assertEqual(result["artifact_id"], 501)
            self.assertEqual(result["source_sha"], SHA)
            self.assertEqual(result["harness_sha"], HEAD)
            self.assertIn("/attempts/2/jobs", endpoints[-1])
            self.assertEqual(calls, 1)

    def test_binary_change_requires_new_build_without_older_fallback(self):
        result, _, calls = self.invoke(*fixture(), changed=True)
        self.assertTrue(result["build_needed"])
        self.assertIsNone(result["artifact_id"])
        self.assertEqual(result["reason"], "binary_inputs_changed")
        self.assertEqual(calls, 0)

    def test_unavailable_or_nonancestor_package_requires_build(self):
        run, artifact, job = fixture()
        artifact["expired"] = True
        result, _, calls = self.invoke(run, artifact, job)
        self.assertEqual(result["reason"], "latest_package_unavailable")
        self.assertEqual(calls, 0)
        artifact["expired"] = False
        result, _, _ = self.invoke(run, artifact, job, ancestor=False)
        self.assertTrue(result["build_needed"])

    def test_run_number_then_original_attempt_orders_latest_successful_package(self):
        run, artifact, job = fixture()
        old = copy.deepcopy(run); old.update(id=36, run_number=11)
        recent = copy.deepcopy(run); recent.update(id=38, run_number=13, conclusion="failure", status="completed")
        earlier = copy.deepcopy(artifact); earlier.update(id=500, name=f"grok-{SHA}-{TARGET}-37-1")
        calls = []
        def inventory(endpoint, key):
            calls.append(endpoint)
            if key == "workflow_runs": return [old, run, recent]
            if "/runs/38/" in endpoint: return []
            if key == "artifacts": return [earlier, artifact]
            if key == "jobs": return [job]
            self.fail(endpoint)
        with mock.patch.object(selection, "newest_runs", return_value=iter([recent, run, old])), \
             mock.patch.object(selection, "inventory", side_effect=inventory), \
             mock.patch.object(selection, "ancestor", return_value=True), \
             mock.patch.object(selection, "binary_inputs_changed", return_value=False), \
             mock.patch.object(selection, "fetch", return_value={"artifact_id": 501, "source_sha": SHA}):
            result = selection.plan(LINE, HEAD, Path.cwd())
        self.assertEqual(result["artifact_id"], 501)
        self.assertIn("/runs/38/artifacts", calls[0])
        self.assertIn("/attempts/2/jobs", calls[-1])
        self.assertFalse(any("/runs/36/" in path for path in calls))

    def test_corrupt_identity_or_archive_fails_instead_of_build_or_fallback(self):
        run, artifact, job = fixture()
        artifact["digest"] = "bad"
        with self.assertRaises(ValueError): self.invoke(run, artifact, job)
        artifact["digest"] = "sha256:" + "c" * 64
        with mock.patch.object(selection, "newest_runs", return_value=iter([run])), \
             mock.patch.object(selection, "inventory", side_effect=lambda endpoint, key: {
            "workflow_runs": [run], "artifacts": [artifact], "jobs": [job]}[key]), \
             mock.patch.object(selection, "ancestor", return_value=True), \
             mock.patch.object(selection, "binary_inputs_changed", return_value=False), \
             mock.patch.object(selection, "fetch", side_effect=ValueError("private error")):
            with self.assertRaises(ValueError): selection.plan(LINE, HEAD, Path.cwd())

    def test_missing_latest_success_package_does_not_fall_back(self):
        run, artifact, job = fixture()
        run["run_attempt"] = 2
        older = copy.deepcopy(run); older.update(id=36, run_number=11)
        with mock.patch.object(selection, "newest_runs", return_value=iter([run])), \
             mock.patch.object(selection, "inventory", side_effect=lambda endpoint, key: {
            "workflow_runs": [older, run], "artifacts": [], "jobs": [job]}[key]), \
             mock.patch.object(selection, "ancestor", return_value=True), \
             mock.patch.object(selection, "fetch") as fetch:
            result = selection.plan(LINE, HEAD, Path.cwd())
            self.assertEqual(result["reason"], "latest_package_unavailable")
            fetch.assert_not_called()

    def test_macos_independently_requires_its_own_successful_package(self):
        run, artifact, job = fixture()
        run["run_attempt"] = 2
        mac = copy.deepcopy(artifact)
        mac.update(id=502, name=f"grok-{SHA}-aarch64-apple-darwin-37-2")
        mac_job = copy.deepcopy(job)
        mac_job.update(id=102, name="Build aarch64-apple-darwin", conclusion="failure")
        for conclusion, build_needed in (("failure", True), ("success", False)):
            mac_job["conclusion"] = conclusion
            with mock.patch.object(selection, "newest_runs", return_value=iter([run])), \
                 mock.patch.object(selection, "inventory", side_effect=lambda endpoint, key: {
                "workflow_runs": [run], "artifacts": [artifact, mac], "jobs": [job, mac_job]}[key]), \
                 mock.patch.object(selection, "ancestor", return_value=True), \
                 mock.patch.object(selection, "binary_inputs_changed", return_value=False), \
                 mock.patch.object(selection, "fetch", return_value={"artifact_id": 501, "source_sha": SHA}) as fetch:
                result = selection.plan(LINE, HEAD, Path.cwd(), include_macos=True)
                self.assertFalse(result["build_needed"])
                self.assertEqual(result["mac_build_needed"], build_needed)
                self.assertEqual(fetch.call_count, 1)  # Mac is metadata-only, never downloaded or executed.

    def test_nonversion_line_builds_without_reuse_or_api_calls(self):
        with mock.patch.object(selection, "api") as api:
            result = selection.plan("grok/other-line", HEAD, Path.cwd())
            self.assertTrue(result["build_needed"])
            self.assertEqual(result["reason"], "no_reusable_version_line")
            api.assert_not_called()

    def test_large_history_stops_on_first_page_and_shares_it_with_macos(self):
        run, artifact, job = fixture()
        run["run_number"] = 2000
        page = [dict(run, id=37 + index, run_number=2000 - index) for index in range(100)]
        mac = dict(artifact, id=502, name=f"grok-{SHA}-aarch64-apple-darwin-37-2")
        mac_job = dict(job, id=102, name="Build aarch64-apple-darwin")
        with mock.patch.object(selection, "api", side_effect=[
            {"total_count": 2000, "workflow_runs": page}, AssertionError("unneeded older page")]) as api, \
             mock.patch.object(selection, "inventory", side_effect=lambda endpoint, key: {
                 "artifacts": [artifact, mac], "jobs": [job, mac_job]}[key]), \
             mock.patch.object(selection, "ancestor", return_value=True), \
             mock.patch.object(selection, "binary_inputs_changed", return_value=False), \
             mock.patch.object(selection, "fetch", return_value={"artifact_id": 501, "source_sha": SHA}):
            result = selection.plan(LINE, HEAD, Path.cwd(), include_macos=True)
        self.assertFalse(result["build_needed"])
        self.assertFalse(result["mac_build_needed"])
        self.assertEqual(api.call_count, 1)

    def test_run_pages_accept_changed_counts_with_complete_ordered_pages(self):
        run, _, _ = fixture()
        first = [dict(run, id=1000 + index, run_number=2000 - index) for index in range(100)]
        second = [dict(run, id=1100 + index, run_number=1900 - index) for index in range(99)]
        with mock.patch.object(selection, "api", side_effect=[
            {"total_count": 200, "workflow_runs": first},
            {"total_count": 199, "workflow_runs": second},
        ]) as api:
            self.assertEqual(list(selection.newest_runs("/runs?branch=line")), first + second)
        self.assertEqual(api.call_count, 2)

    def test_run_search_errors_are_not_end_of_history(self):
        run, _, _ = fixture()
        cases = [
            {"total_count": 2, "workflow_runs": [run]},
            {"total_count": 2, "workflow_runs": [run, run]},
            {"total_count": 2, "workflow_runs": [run, dict(run, id=38, run_number=13)]},
        ]
        for data in cases:
            with self.subTest(data=data), mock.patch.object(selection, "api", return_value=data):
                with self.assertRaises(ValueError): list(selection.newest_runs("/runs?branch=line"))
        page = [dict(run, id=37 + index, run_number=2000 - index) for index in range(100)]
        with mock.patch.object(selection, "MAX_RUN_PAGES", 1), \
             mock.patch.object(selection, "api", return_value={"total_count": 2000, "workflow_runs": page}):
            with self.assertRaisesRegex(ValueError, "run_budget"):
                list(selection.newest_runs("/runs?branch=line"))
        with mock.patch.object(selection, "api", side_effect=OSError("network unavailable")):
            with self.assertRaises(OSError): list(selection.newest_runs("/runs?branch=line"))

    def test_unavailable_history_or_diff_fails_without_build_decision(self):
        run, artifact, job = fixture()
        for failing_function in ("ancestor", "binary_inputs_changed"):
            with self.subTest(function=failing_function), \
                 mock.patch.object(selection, "newest_runs", return_value=iter([run])), \
                 mock.patch.object(selection, "inventory", side_effect=lambda endpoint, key: {
                     "artifacts": [artifact], "jobs": [job]}[key]), \
                 mock.patch.object(selection, "ancestor", return_value=True), \
                 mock.patch.object(selection, "binary_inputs_changed", return_value=False), \
                 mock.patch.object(selection, failing_function, side_effect=ValueError("history unavailable")), \
                 mock.patch.object(selection, "fetch") as fetch:
                with self.assertRaises(ValueError): selection.plan(LINE, HEAD, Path.cwd())
                fetch.assert_not_called()

    def test_error_diagnostic_does_not_publish_a_build_decision_or_private_error(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output, github_output = root / "decision.json", root / "github-output"
            for stage in ("run_budget", "source_diff"):
                with mock.patch.object(selection, "plan", side_effect=selection.BuildSelectionError(stage)), \
                     mock.patch.object(selection.sys, "argv", ["build_selection.py", "--line", LINE,
                         "--harness-sha", HEAD, "--output", str(output), "--github-output", str(github_output)]), \
                     mock.patch.object(selection.sys, "stderr"):
                    self.assertEqual(selection.main(), 1)
                self.assertEqual(json.loads(output.read_text()), {"state": "error", "reason": stage})
                self.assertFalse(github_output.exists())

    def test_inventory_requires_complete_unique_bounded_pages(self):
        with mock.patch.object(selection, "api", side_effect=[
            {"total_count": 2, "jobs": [{"id": 1}]}, {"total_count": 2, "jobs": [{"id": 2}]}]):
            self.assertEqual(selection.inventory("/jobs", "jobs"), [{"id": 1}, {"id": 2}])
        for data in ({"total_count": 1001, "jobs": []}, {"total_count": 1, "jobs": []},
                     {"total_count": 2, "jobs": [{"id": 1}, {"id": 1}]}):
            with mock.patch.object(selection, "api", return_value=data):
                with self.assertRaises(ValueError): selection.inventory("/jobs", "jobs")


if __name__ == "__main__":
    unittest.main()
