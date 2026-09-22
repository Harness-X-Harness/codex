"""Unit tests for the grok publish gate."""

import re
import subprocess
import sys
import tempfile
import unittest
from io import StringIO
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))

import release

ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = ROOT / ".github/workflows/grok.yml"
MINIMAL_PUSH_PATHS = [
    "**",
    "!docs/**",
    "!*.md",
    "!.github/**",
    ".github/workflows/grok.yml",
]
REPO = "owner/name"
RUN_ID = "1"
REAL_PUSH_PATHS_REINCLUDE = ".github/workflows/grok.yml"


def _workflow_yaml(patterns: list[str]) -> str:
    lines = ["on:", "  push:", "    paths:"]
    lines.extend(f"      - '{pattern}'" for pattern in patterns)
    return "\n".join(lines) + "\n"


def _git(repo: Path, *args: str) -> str:
    try:
        return subprocess.check_output(
            [
                "git",
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "-c",
                "commit.gpgsign=false",
                "-C",
                str(repo),
                *args,
            ],
            text=True,
            stderr=subprocess.PIPE,
        )
    except subprocess.CalledProcessError as error:
        raise AssertionError(error.stderr or error.stdout) from error


def _write(path: Path, content: str = "x\n") -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def _commit(repo: Path, message: str) -> str:
    _git(repo, "add", "-A")
    _git(repo, "commit", "-m", message)
    return _git(repo, "rev-parse", "HEAD").strip()


def _init_repo(root: Path, patterns: list[str] | None = None) -> None:
    _git(root, "init", "-b", "main")
    _write(root / release.PROOF_WORKFLOW, _workflow_yaml(patterns or MINIMAL_PUSH_PATHS))
    _write(root / "src/app.rs")


def _run_payload(
    sha: str,
    *,
    workflow: str = "grok",
    event: str = "push",
    status: str = "completed",
    conclusion: str | None = "success",
    branch: str = "grok/rust-v0.154.0",
    jobs: list[dict] | None = None,
) -> dict:
    if jobs is None:
        jobs = [{"name": release.LIVE_JOB, "conclusion": "success"}]
    return {
        "workflowName": workflow,
        "event": event,
        "status": status,
        "conclusion": conclusion,
        "headBranch": branch,
        "headSha": sha,
        "jobs": jobs,
    }


def _artifacts(sha: str, omit: frozenset[str] = frozenset()) -> list[dict[str, str]]:
    return [
        {"name": release.artifact_name(sha, target)}
        for target in release.TARGETS
        if target not in omit
    ]


def _gh_json(run: dict, ref_sha: str, artifacts: list[dict[str, str]]):
    def fake(*args: str):
        if args[:2] == ("run", "view"):
            return run
        if args[0] == "api" and "/git/ref/heads/" in args[1]:
            return {"object": {"sha": ref_sha}}
        if args[0] == "api" and args[1].endswith("/artifacts"):
            return {"artifacts": artifacts}
        raise AssertionError(args)

    return fake


class ProofPushPathsTests(unittest.TestCase):
    def test_proof_workflow_builds_each_publish_target(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")
        built = set(re.findall(r"(?m)^\s+(?:- )?target: ([a-z0-9_-]+)$", text))
        self.assertEqual(built, set(release.TARGETS))

    def test_reads_real_grok_yml(self) -> None:
        # The workflow is the authority for the list; assert its shape, not its rows.
        patterns = release.proof_push_paths(WORKFLOW)
        self.assertEqual(patterns[0], "**")
        self.assertIn("!.github/**", patterns)
        self.assertGreater(
            patterns.index(REAL_PUSH_PATHS_REINCLUDE), patterns.index("!.github/**")
        )

    def test_reads_minimal_fixture(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "grok.yml"
            path.write_text(_workflow_yaml(MINIMAL_PUSH_PATHS), encoding="utf-8")
            self.assertEqual(release.proof_push_paths(path), MINIMAL_PUSH_PATHS)

    def test_real_filter_selects_proof_inputs(self) -> None:
        patterns = release.proof_push_paths(WORKFLOW)
        self.assertTrue(release.is_proof_input("grok/release.py", patterns))
        self.assertTrue(release.is_proof_input("LICENSE", patterns))
        self.assertTrue(release.is_proof_input(".github/workflows/grok.yml", patterns))
        self.assertTrue(
            release.is_proof_input(".github/actions/build-grok/action.yml", patterns)
        )
        self.assertFalse(release.is_proof_input("grok/docs/release.md", patterns))
        self.assertFalse(release.is_proof_input("grok/facts/facts.go", patterns))
        self.assertFalse(release.is_proof_input("README.md", patterns))
        self.assertFalse(release.is_proof_input(".github/workflows/other.yml", patterns))
        self.assertFalse(
            release.is_proof_input(".github/workflows/grok-facts.yml", patterns)
        )


class IsProofInputTests(unittest.TestCase):
    def test_starstar_matches_every_path(self) -> None:
        self.assertTrue(release.is_proof_input("a", ["**"]))
        self.assertTrue(release.is_proof_input("a/b/c.rs", ["**"]))

    def test_star_matches_one_segment(self) -> None:
        self.assertTrue(release.is_proof_input("foo.rs", ["*.rs"]))
        self.assertFalse(release.is_proof_input("src/foo.rs", ["*.rs"]))
        self.assertTrue(release.is_proof_input("src/foo.rs", ["src/*"]))
        self.assertFalse(release.is_proof_input("src/sub/foo.rs", ["src/*"]))

    def test_question_matches_one_non_slash_character(self) -> None:
        self.assertTrue(release.is_proof_input("a.rs", ["?.rs"]))
        self.assertFalse(release.is_proof_input("ab.rs", ["?.rs"]))
        self.assertFalse(release.is_proof_input("a/b.rs", ["?.rs"]))

    def test_negate_unselects_a_match(self) -> None:
        patterns = ["**", "!docs/**"]
        self.assertFalse(release.is_proof_input("docs/a.md", patterns))
        self.assertTrue(release.is_proof_input("src/a.rs", patterns))

    def test_later_pattern_re_includes(self) -> None:
        patterns = ["**", "!.github/**", ".github/workflows/grok.yml"]
        self.assertTrue(release.is_proof_input(".github/workflows/grok.yml", patterns))
        self.assertFalse(release.is_proof_input(".github/other.yml", patterns))

    def test_later_matching_pattern_wins(self) -> None:
        self.assertFalse(release.is_proof_input("foo.md", ["*.md", "!*.md"]))
        self.assertTrue(release.is_proof_input("foo.md", ["!*.md", "*.md"]))

    def test_unmatched_path_is_not_selected(self) -> None:
        self.assertFalse(release.is_proof_input("foo", ["bar"]))


class RequireHeadProvenTests(unittest.TestCase):
    def setUp(self) -> None:
        directory = self.enterContext(tempfile.TemporaryDirectory())
        self.root = Path(directory)
        _init_repo(self.root)

    def test_equal_head(self) -> None:
        sha = _commit(self.root, "proof")
        self.assertEqual(release._require_head_proven(self.root, sha), sha)

    def test_inert_only_delta(self) -> None:
        sha = _commit(self.root, "proof")
        _write(self.root / "README.md", "docs\n")
        _write(self.root / "docs/note.md", "note\n")
        head = _commit(self.root, "inert")
        self.assertEqual(release._require_head_proven(self.root, sha), head)

    def test_proof_input_delta(self) -> None:
        sha = _commit(self.root, "proof")
        _write(self.root / "src/app.rs", "changed\n")
        head = _commit(self.root, "unproven")
        with self.assertRaises(SystemExit) as ctx:
            release._require_head_proven(self.root, sha)
        self.assertIn(head, str(ctx.exception))
        self.assertIn(sha, str(ctx.exception))
        self.assertIn("src/app.rs", str(ctx.exception))

    def test_non_ancestor(self) -> None:
        parent = _commit(self.root, "parent")
        _write(self.root / "src/app.rs", "child\n")
        child = _commit(self.root, "child")
        _git(self.root, "checkout", "--detach", parent)
        with self.assertRaises(SystemExit) as ctx:
            release._require_head_proven(self.root, child)
        self.assertIn("does not descend", str(ctx.exception))
        self.assertIn(child, str(ctx.exception))

    def test_rename_out_of_a_proof_path(self) -> None:
        sha = _commit(self.root, "proof")
        (self.root / "docs").mkdir()
        _git(self.root, "mv", "src/app.rs", "docs/app.md")
        head = _commit(self.root, "rename out")
        with self.assertRaises(SystemExit) as ctx:
            release._require_head_proven(self.root, sha)
        self.assertIn("src/app.rs", str(ctx.exception))
        self.assertIn(head, str(ctx.exception))


class RequireProofTests(unittest.TestCase):
    def setUp(self) -> None:
        self.enterContext(
            patch.object(release, "_gh", side_effect=AssertionError("network"))
        )
        directory = self.enterContext(tempfile.TemporaryDirectory())
        self.root = Path(directory)
        _init_repo(self.root)
        self.sha = _commit(self.root, "proof")

    def _patch_gh(
        self,
        run: dict,
        ref_sha: str | None = None,
        artifacts: list[dict[str, str]] | None = None,
    ):
        return patch.object(
            release,
            "_gh_json",
            side_effect=_gh_json(
                run,
                ref_sha if ref_sha is not None else self.sha,
                artifacts if artifacts is not None else _artifacts(self.sha),
            ),
        )

    def test_not_grok_workflow(self) -> None:
        run = _run_payload(self.sha, workflow="CI")
        with self._patch_gh(run), self.assertRaises(SystemExit) as ctx:
            release._require_proof(REPO, RUN_ID, self.root)
        self.assertIn("is not the grok proof workflow", str(ctx.exception))

    def test_not_a_push_proof(self) -> None:
        run = _run_payload(self.sha, event="pull_request")
        with self._patch_gh(run), self.assertRaises(SystemExit) as ctx:
            release._require_proof(REPO, RUN_ID, self.root)
        self.assertIn("is not a push proof", str(ctx.exception))

    def test_cancelled_run(self) -> None:
        run = _run_payload(self.sha, conclusion="cancelled")
        with self._patch_gh(run), self.assertRaises(SystemExit) as ctx:
            release._require_proof(REPO, RUN_ID, self.root)
        self.assertEqual(str(ctx.exception), f"run {RUN_ID} is completed/cancelled")

    def test_incomplete_run(self) -> None:
        run = _run_payload(self.sha, status="in_progress", conclusion=None)
        with self._patch_gh(run), self.assertRaises(SystemExit) as ctx:
            release._require_proof(REPO, RUN_ID, self.root)
        self.assertEqual(str(ctx.exception), f"run {RUN_ID} is in_progress/None")

    def test_ref_is_not_a_product_line(self) -> None:
        run = _run_payload(self.sha, branch="main")
        with self._patch_gh(run), self.assertRaises(SystemExit) as ctx:
            release._require_proof(REPO, RUN_ID, self.root)
        self.assertIn("ref must be grok/main or start with", str(ctx.exception))

    def test_version_is_not_a_dotted_release_number(self) -> None:
        run = _run_payload(self.sha, branch="grok/rust-v0.154.0/extra")
        with self._patch_gh(run), self.assertRaises(SystemExit) as ctx:
            release._require_proof(REPO, RUN_ID, self.root)
        self.assertIn("version must be a dotted release number", str(ctx.exception))

    def test_head_not_proven(self) -> None:
        _write(self.root / "src/app.rs", "changed\n")
        _commit(self.root, "unproven")
        run = _run_payload(self.sha)
        with self._patch_gh(run), self.assertRaises(SystemExit) as ctx:
            release._require_proof(REPO, RUN_ID, self.root)
        self.assertIn("changes proof inputs", str(ctx.exception))

    def test_branch_moved(self) -> None:
        run = _run_payload(self.sha)
        other = "ab" * 20
        with self._patch_gh(run, ref_sha=other), self.assertRaises(SystemExit) as ctx:
            release._require_proof(REPO, RUN_ID, self.root)
        self.assertIn(f"branch grok/rust-v0.154.0 is {other}", str(ctx.exception))
        self.assertIn(f"checkout is {self.sha}", str(ctx.exception))

    def test_live_missing(self) -> None:
        run = _run_payload(self.sha, jobs=[])
        with self._patch_gh(run), self.assertRaises(SystemExit) as ctx:
            release._require_proof(REPO, RUN_ID, self.root)
        self.assertEqual(str(ctx.exception), f"{release.LIVE_JOB} did not succeed")

    def test_live_failed(self) -> None:
        run = _run_payload(
            self.sha, jobs=[{"name": release.LIVE_JOB, "conclusion": "failure"}]
        )
        with self._patch_gh(run), self.assertRaises(SystemExit) as ctx:
            release._require_proof(REPO, RUN_ID, self.root)
        self.assertEqual(str(ctx.exception), f"{release.LIVE_JOB} did not succeed")

    def test_missing_artifacts(self) -> None:
        run = _run_payload(self.sha)
        missing = _artifacts(self.sha, omit=frozenset({"aarch64-apple-darwin"}))
        with self._patch_gh(run, artifacts=missing), self.assertRaises(SystemExit) as ctx:
            release._require_proof(REPO, RUN_ID, self.root)
        self.assertIn("missing artifacts", str(ctx.exception))
        self.assertIn(
            release.artifact_name(self.sha, "aarch64-apple-darwin"), str(ctx.exception)
        )

    def test_success(self) -> None:
        run = _run_payload(self.sha)
        with self._patch_gh(run):
            branch, version, sha = release._require_proof(REPO, RUN_ID, self.root)
        self.assertEqual(
            (branch, version, sha), ("grok/rust-v0.154.0", "0.154.0", self.sha)
        )

    def test_success_on_product_branch(self) -> None:
        run = _run_payload(self.sha, branch="grok/main")
        with self._patch_gh(run):
            branch, version, sha = release._require_proof(REPO, RUN_ID, self.root)
        self.assertEqual((branch, version, sha), ("grok/main", "main", self.sha))


class CheckTests(unittest.TestCase):
    def setUp(self) -> None:
        self.enterContext(
            patch.object(release, "_gh", side_effect=AssertionError("network"))
        )
        directory = self.enterContext(tempfile.TemporaryDirectory())
        self.root = Path(directory)
        _init_repo(self.root)
        self.sha = _commit(self.root, "proof")

    def test_prints_publishable(self) -> None:
        run = _run_payload(self.sha)
        fake = _gh_json(run, self.sha, _artifacts(self.sha))
        with (
            patch.object(release, "_gh_json", side_effect=fake),
            patch.object(
                release, "_stage_channel", side_effect=AssertionError("download")
            ),
            patch("sys.stdout", new_callable=StringIO) as out,
        ):
            release.check(REPO, RUN_ID, self.root)
        self.assertEqual(
            out.getvalue(),
            f"publishable grok-v0.154.0 from {self.sha} at {self.sha}\n",
        )

    def test_prints_refusal(self) -> None:
        run = _run_payload(self.sha, conclusion="cancelled")
        fake = _gh_json(run, self.sha, _artifacts(self.sha))
        with (
            patch.object(release, "_gh_json", side_effect=fake),
            self.assertRaises(SystemExit) as ctx,
        ):
            release.check(REPO, RUN_ID, self.root)
        self.assertEqual(str(ctx.exception), f"run {RUN_ID} is completed/cancelled")

    def test_main_dispatches_check(self) -> None:
        checkout = Path(release.__file__).resolve().parent.parent
        with (
            patch.object(
                sys, "argv", ["release.py", "check", "--run-id", "9", "--repo", REPO]
            ),
            patch.object(release, "check") as mocked,
        ):
            release.main()
        mocked.assert_called_once_with(REPO, "9", checkout)


if __name__ == "__main__":
    unittest.main()
