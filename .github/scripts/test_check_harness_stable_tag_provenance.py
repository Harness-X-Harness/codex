#!/usr/bin/env python3
"""Fixture tests for harness stable-tag provenance mapping and ancestry."""

import subprocess
import tempfile
import unittest
from pathlib import Path

from check_harness_stable_tag_provenance import ProvenanceError
from check_harness_stable_tag_provenance import check_provenance
from check_harness_stable_tag_provenance import expected_stock_tag


class ExpectedStockTagTest(unittest.TestCase):
    def test_derives_tag_from_harness_branch_without_hard_coded_version(self) -> None:
        self.assertEqual(expected_stock_tag("harness/rust-v0.153.4"), "rust-v0.153.4")
        self.assertEqual(expected_stock_tag("harness/rust-v0.200.1"), "rust-v0.200.1")
        self.assertEqual(
            expected_stock_tag("refs/heads/harness/rust-v0.99.0"),
            "rust-v0.99.0",
        )

    def test_wrong_branch_to_tag_mapping_is_not_rewritten(self) -> None:
        self.assertEqual(expected_stock_tag("harness/other-tag"), "other-tag")
        self.assertNotEqual(expected_stock_tag("harness/other-tag"), "rust-v0.153.4")

    def test_non_harness_and_invalid_branches_fail(self) -> None:
        for branch in (
            "release/rust-v0.153.4",
            "main",
            "harness/",
            "harness/rust-v0.153.4/extra",
            "harness/bad tag",
        ):
            with self.subTest(branch=branch):
                with self.assertRaises(ProvenanceError):
                    expected_stock_tag(branch)


class AncestryTest(unittest.TestCase):
    def test_commits_ahead_of_matching_tag_pass(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            repo = self.init_repo(Path(temp_dir))
            tag = self.commit(repo, "stock.txt", "stock")
            self.run_git(repo, "tag", "rust-v0.200.1", tag)
            self.commit(repo, "host.txt", "host goal")
            report, ok = check_provenance(
                repo,
                "harness/rust-v0.200.1",
                ("HEAD",),
            )
            self.assertTrue(ok)
            self.assertIn("stock_tag=rust-v0.200.1", report)
            self.assertIn("ancestor=yes", report)

    def test_missing_tag_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            repo = self.init_repo(Path(temp_dir))
            self.commit(repo, "stock.txt", "stock")
            report, ok = check_provenance(repo, "harness/other-tag", ("HEAD",))
            self.assertFalse(ok)
            self.assertIn("stock_tag=other-tag", report)
            self.assertIn("tag_sha=missing", report)

    def test_branch_name_is_not_accepted_as_the_stock_tag(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            repo = self.init_repo(Path(temp_dir))
            self.commit(repo, "stock.txt", "stock")
            self.run_git(repo, "branch", "other-tag")
            report, ok = check_provenance(repo, "harness/other-tag", ("HEAD",))
            self.assertFalse(ok)
            self.assertIn("tag_sha=missing", report)

    def test_tag_that_is_not_an_ancestor_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            repo = self.init_repo(Path(temp_dir))
            self.commit(repo, "stock.txt", "stock")
            self.run_git(repo, "switch", "-c", "side")
            other = self.commit(repo, "other.txt", "other line")
            self.run_git(repo, "tag", "rust-v0.200.1", other)
            self.run_git(repo, "switch", "-")
            self.commit(repo, "host.txt", "host goal")
            report, ok = check_provenance(repo, "harness/rust-v0.200.1", ("HEAD",))
            self.assertFalse(ok)
            self.assertIn("ancestor=no", report)

    def init_repo(self, root: Path) -> Path:
        self.run_git(root, "init", "--initial-branch=main")
        self.run_git(root, "config", "user.name", "Test User")
        self.run_git(root, "config", "user.email", "test@example.com")
        return root

    def commit(self, repo: Path, path: str, contents: str) -> str:
        (repo / path).write_text(contents)
        self.run_git(repo, "add", path)
        self.run_git(repo, "commit", "-m", contents)
        return self.run_git(repo, "rev-parse", "HEAD")

    def run_git(self, repo: Path, *args: str) -> str:
        return subprocess.check_output(
            ["git", *args],
            cwd=repo,
            stderr=subprocess.PIPE,
            text=True,
        ).strip()


if __name__ == "__main__":
    unittest.main()
