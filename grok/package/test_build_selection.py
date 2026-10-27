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
            self.assertTrue(selection.binary_inputs_changed(root, base, head))
            self.assertFalse(selection.binary_inputs_changed(root, git("rev-parse", "HEAD~1"), head))
            (root / "docs").mkdir()
            (root / "codex-rs/runtime.rs").rename(root / "docs/runtime.md")
            git("add", "-A"); git("commit", "-qm", "move binary to docs"); moved = git("rev-parse", "HEAD")
            self.assertTrue(selection.binary_inputs_changed(root, head, moved))
            with mock.patch("select_artifact.ensure_source_commit", side_effect=ValueError("unavailable")):
                with self.assertRaises(ValueError):
                    selection.binary_inputs_changed(root, "0" * 40, moved)

    def invoke(self, run, artifact, job, *, changed=False):
        endpoints = []
        def inventory(endpoint, key):
            endpoints.append(endpoint)
            return {"workflow_runs": [run], "artifacts": [artifact], "jobs": [job]}[key]
        with mock.patch.object(selection, "recent_runs", return_value=[run]), \
             mock.patch.object(selection, "inventory", side_effect=inventory), \
             mock.patch.object(selection, "binary_inputs_changed", return_value=changed), \
             mock.patch.object(selection, "resolve_producer", wraps=selection.resolve_producer) as fetch:
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

    def test_no_content_matching_package_requires_new_build(self):
        result, _, calls = self.invoke(*fixture(), changed=True)
        self.assertTrue(result["build_needed"])
        self.assertIsNone(result["artifact_id"])
        self.assertEqual(result["reason"], "no_matching_recent_artifact")
        self.assertEqual(calls, 1)

    def test_unavailable_package_requires_build(self):
        run, artifact, job = fixture()
        artifact["expired"] = True
        result, _, calls = self.invoke(run, artifact, job)
        self.assertEqual(result["reason"], "no_matching_recent_artifact")
        self.assertEqual(calls, 0)

    def test_pr_merge_source_reuses_same_tree_without_ancestry_and_rebuilds_changed_inputs(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            def git(*args):
                return subprocess.check_output(["git", *args], cwd=root, stderr=subprocess.DEVNULL, text=True).strip()
            git("init", "-q"); git("config", "user.name", "fixture"); git("config", "user.email", "fixture@example.invalid")
            (root / "codex-rs").mkdir(); (root / "codex-rs/runtime.rs").write_text("binary")
            git("add", "."); git("commit", "-qm", "base"); base = git("rev-parse", "HEAD")
            (root / "README.md").write_text("docs")
            git("add", "."); git("commit", "-qm", "producer merge source"); source = git("rev-parse", "HEAD")
            git("checkout", "-q", "-B", "current", base)
            (root / "README.md").write_text("docs")
            git("add", "."); git("commit", "-qm", "rebased current source"); current = git("rev-parse", "HEAD")
            self.assertNotEqual(source, current)
            self.assertEqual(git("rev-parse", source + "^{tree}"), git("rev-parse", current + "^{tree}"))
            self.assertEqual(subprocess.run(["git", "merge-base", "--is-ancestor", source, current], cwd=root).returncode, 1)
            run, artifact, job = fixture()
            run.update(event="pull_request", head_branch="feature")
            artifact["name"] = f"grok-{source}-{TARGET}-37-2"
            for changed in (False, True):
                if changed:
                    (root / "codex-rs/runtime.rs").write_text("new binary input")
                    git("add", "."); git("commit", "-qm", "binary change"); current = git("rev-parse", "HEAD")
                with mock.patch.object(selection, "recent_runs", return_value=[run]), \
                     mock.patch.object(selection, "inventory", side_effect=lambda endpoint, key: {
                         "artifacts": [artifact], "jobs": [job]}[key]), \
                     mock.patch.object(selection, "resolve_producer", return_value={"source_sha": source, "producer_line": LINE}) as auth, \
                     mock.patch("fetch_artifact.fetch", side_effect=AssertionError("planner must not download binary")):
                    result = selection.plan(LINE, current, root)
                self.assertEqual(result["build_needed"], changed)
                if not changed:
                    self.assertEqual(result["source_sha"], source)
                    self.assertEqual(result["artifact_id"], 501)
                auth.assert_called_once()

    def test_newer_fork_run_is_excluded_without_blocking_trusted_candidate(self):
        run, artifact, job = fixture()
        fork = copy.deepcopy(run)
        fork.update(id=38, run_number=13, event="pull_request")
        fork["head_repository"]["full_name"] = "other-owner/codex"
        with mock.patch.object(selection, "recent_runs", return_value=[fork, run]), \
             mock.patch.object(selection, "inventory", side_effect=lambda endpoint, key: {
                 "artifacts": [artifact], "jobs": [job]}[key]), \
             mock.patch.object(selection, "binary_inputs_changed", return_value=False):
            result = selection.plan(LINE, HEAD, Path.cwd())
        self.assertFalse(result["build_needed"])
        self.assertEqual(result["artifact_id"], 501)

    def test_run_number_then_original_attempt_orders_latest_successful_package(self):
        run, artifact, job = fixture()
        old = copy.deepcopy(run); old.update(id=36, run_number=11)
        recent = copy.deepcopy(run); recent.update(id=38, run_number=13, event="pull_request", head_branch="current-pr",
                                                pull_requests=[], conclusion=None, status="in_progress")
        earlier = copy.deepcopy(artifact); earlier.update(id=500, name=f"grok-{SHA}-{TARGET}-37-1")
        calls = []
        def inventory(endpoint, key):
            calls.append(endpoint)
            if key == "workflow_runs": return [old, run, recent]
            if "/runs/38/" in endpoint: return []
            if key == "artifacts": return [earlier, artifact]
            if key == "jobs": return [job]
            self.fail(endpoint)
        with mock.patch.object(selection, "recent_runs", return_value=[recent, run, old]), \
             mock.patch.object(selection, "inventory", side_effect=inventory), \
             mock.patch.object(selection, "binary_inputs_changed", return_value=False), \
             mock.patch.object(selection, "resolve_producer", wraps=selection.resolve_producer):
            result = selection.plan(LINE, HEAD, Path.cwd())
        self.assertEqual(result["artifact_id"], 501)
        self.assertIn("/runs/38/artifacts", calls[0])
        self.assertIn("/attempts/2/jobs", calls[-1])
        self.assertFalse(any("/runs/36/" in path for path in calls))

    def test_corrupt_identity_or_source_authentication_fails_instead_of_build_or_fallback(self):
        run, artifact, job = fixture()
        artifact["digest"] = "bad"
        with self.assertRaises(ValueError): self.invoke(run, artifact, job)
        artifact["digest"] = "sha256:" + "c" * 64
        with mock.patch.object(selection, "recent_runs", return_value=[run]), \
             mock.patch.object(selection, "inventory", side_effect=lambda endpoint, key: {
            "workflow_runs": [run], "artifacts": [artifact], "jobs": [job]}[key]), \
             mock.patch.object(selection, "binary_inputs_changed", return_value=False), \
             mock.patch.object(selection, "resolve_producer", side_effect=ValueError("private error")):
            with self.assertRaises(ValueError): selection.plan(LINE, HEAD, Path.cwd())

    def test_newer_unrelated_pr_and_unavailable_packages_do_not_hide_content_match(self):
        run, artifact, job = fixture()
        newer = copy.deepcopy(run)
        newer.update(id=38, run_number=13, event="pull_request", head_branch="other-feature")
        other_source = "d" * 40
        other_artifact = dict(artifact, id=502, name=f"grok-{other_source}-{TARGET}-38-2",
                              workflow_run={"id": 38, "head_sha": SHA})
        other_job = dict(job, id=102, run_id=38)
        for availability in ("different_contents", "expired", "missing"):
            with self.subTest(candidate=availability):
                other_artifact["expired"] = availability == "expired"
                def inventory(endpoint, key):
                    if "/runs/38/" in endpoint:
                        return ([] if availability == "missing" else [other_artifact]) if key == "artifacts" else [other_job]
                    return [artifact] if key == "artifacts" else [job]
                with mock.patch.object(selection, "recent_runs", return_value=[newer, run]), \
                     mock.patch.object(selection, "inventory", side_effect=inventory), \
                     mock.patch.object(selection, "resolve_producer", side_effect=lambda root, run, jobs, artifact, *args, **kwargs: {
                         "source_sha": other_source if run["id"] == 38 else SHA, "producer_line": LINE}), \
                     mock.patch.object(selection, "binary_inputs_changed", side_effect=lambda root, source, current: source == other_source):
                    result = selection.plan(LINE, HEAD, Path.cwd())
                self.assertFalse(result["build_needed"])
                self.assertEqual(result["artifact_id"], 501)
                self.assertEqual(result["source_sha"], SHA)

    def test_macos_independently_requires_its_own_successful_package(self):
        run, artifact, job = fixture()
        run["run_attempt"] = 2
        mac = copy.deepcopy(artifact)
        mac.update(id=502, name=f"grok-{SHA}-aarch64-apple-darwin-37-2")
        mac_job = copy.deepcopy(job)
        mac_job.update(id=102, name="Build aarch64-apple-darwin", conclusion="failure")
        for conclusion, build_needed in (("failure", True), ("success", False)):
            mac_job["conclusion"] = conclusion
            with mock.patch.object(selection, "recent_runs", return_value=[run]), \
                 mock.patch.object(selection, "inventory", side_effect=lambda endpoint, key: {
                "workflow_runs": [run], "artifacts": [artifact, mac], "jobs": [job, mac_job]}[key]), \
                     mock.patch.object(selection, "binary_inputs_changed", return_value=False), \
                 mock.patch.object(selection, "resolve_producer", wraps=selection.resolve_producer) as fetch:
                result = selection.plan(LINE, HEAD, Path.cwd(), include_macos=True)
                self.assertFalse(result["build_needed"])
                self.assertEqual(result["mac_build_needed"], build_needed)
                self.assertEqual(fetch.call_count, 1 if build_needed else 2)  # Both targets authenticate metadata only.

    def test_nonversion_line_builds_without_reuse_or_api_calls(self):
        with mock.patch.object(selection, "api") as api:
            result = selection.plan("grok/other-line", HEAD, Path.cwd())
            self.assertTrue(result["build_needed"])
            self.assertEqual(result["reason"], "no_reusable_version_line")
            api.assert_not_called()

    def test_recent_window_is_read_once_for_both_platforms(self):
        run, artifact, job = fixture()
        run["run_number"] = 2000
        page = [dict(run, id=37 + index, run_number=2000 - index) for index in range(100)]
        mac = dict(artifact, id=502, name=f"grok-{SHA}-aarch64-apple-darwin-37-2")
        mac_job = dict(job, id=102, name="Build aarch64-apple-darwin")
        with mock.patch.object(selection, "api", side_effect=[
            {"total_count": 2000, "workflow_runs": page}, AssertionError("unneeded older page")]) as api, \
             mock.patch.object(selection, "inventory", side_effect=lambda endpoint, key: {
                 "artifacts": [artifact, mac], "jobs": [job, mac_job]}[key]), \
             mock.patch.object(selection, "binary_inputs_changed", return_value=False), \
             mock.patch.object(selection, "resolve_producer", wraps=selection.resolve_producer):
            result = selection.plan(LINE, HEAD, Path.cwd(), include_macos=True)
        self.assertFalse(result["build_needed"])
        self.assertFalse(result["mac_build_needed"])
        self.assertEqual(api.call_count, 1)

    def test_recent_window_checks_only_this_page_not_full_history(self):
        run, _, _ = fixture()
        with mock.patch.object(selection, "api", return_value={"total_count": 1, "workflow_runs": [run]}) as api:
            self.assertEqual(selection.recent_runs("/runs"), [run])
            api.assert_called_once_with("/runs?per_page=100&page=1")
        with mock.patch.object(selection, "api", return_value={"total_count": 0, "workflow_runs": []}):
            self.assertEqual(selection.recent_runs("/runs"), [])
        with mock.patch.object(selection, "api", return_value={"total_count": 100, "workflow_runs": []}):
            with self.assertRaises(selection.BuildSelectionError): selection.recent_runs("/runs")
        for rows in ([run, run], [run, dict(run, id=38, run_number=13)]):
            with mock.patch.object(selection, "api", return_value={"total_count": len(rows), "workflow_runs": rows}):
                with self.assertRaises(ValueError): selection.recent_runs("/runs")
        with mock.patch.object(selection, "api", side_effect=OSError("network unavailable")):
            with self.assertRaises(OSError): selection.recent_runs("/runs")

    def test_unavailable_diff_fails_without_build_decision(self):
        run, artifact, job = fixture()
        with mock.patch.object(selection, "recent_runs", return_value=[run]), \
             mock.patch.object(selection, "inventory", side_effect=lambda endpoint, key: {
                 "artifacts": [artifact], "jobs": [job]}[key]), \
             mock.patch.object(selection, "binary_inputs_changed", side_effect=ValueError("history unavailable")):
            with self.assertRaises(selection.BuildSelectionError): selection.plan(LINE, HEAD, Path.cwd())

    def test_error_diagnostic_does_not_publish_a_build_decision_or_private_error(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output, github_output = root / "decision.json", root / "github-output"
            for stage in ("run_window", "source_diff"):
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
