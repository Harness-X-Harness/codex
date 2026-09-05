import contextlib
import io
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

from grokex import seam_series


def git(repo: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=repo, check=True, capture_output=True, text=True
    ).stdout.strip()


def commit_all(repo: Path, message: str) -> str:
    git(repo, "add", "-A")
    git(
        repo,
        "-c",
        "user.name=Seam Test",
        "-c",
        "user.email=seam@example.invalid",
        "commit",
        "-q",
        "-m",
        message,
    )
    return git(repo, "rev-parse", "HEAD")


class SeamAssignmentTest(unittest.TestCase):
    def test_rules_match_prefixes_and_exact_paths(self) -> None:
        cases = {
            ("codex-rs/vendor/", "codex-rs/vendor/BUILD.bazel"): True,
            (".github/workflows/grokex-", ".github/workflows/grokex-live.yml"): True,
            ("grokex/grokex", "grokex/grokex"): True,
            ("grokex/grokex", "grokex/grokex.ps1"): False,
            ("codex-rs/core/src/tools/router.rs", "codex-rs/core/src/tools/router/x.rs"): False,
        }
        for (rule, path), expected in cases.items():
            with self.subTest(rule=rule, path=path):
                self.assertEqual(seam_series.path_owned_by(rule, path), expected)

    def test_first_owning_seam_wins_and_unowned_paths_are_reported(self) -> None:
        patches = [
            {"name": "a", "paths": ["x/"]},
            {"name": "b", "paths": ["x/y.rs", "z.rs"]},
        ]

        assignment, unowned = seam_series.assign(patches, ["x/y.rs", "z.rs", "q.rs"])

        self.assertEqual(dict(assignment), {"a": ["x/y.rs"], "b": ["z.rs"]})
        self.assertEqual(unowned, ["q.rs"])

    def test_shipped_series_is_well_formed(self) -> None:
        patches = seam_series.load_series()

        self.assertEqual(len(patches), 10)
        self.assertEqual(len({patch["name"] for patch in patches}), len(patches))


class SeamSeriesRoundTripTest(unittest.TestCase):
    def test_export_reproduces_head_tree_and_applies_with_git_am(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            repo = Path(scratch) / "repo"
            repo.mkdir()
            git(repo, "init", "-q", "-b", "main")
            (repo / "core").mkdir()
            (repo / "core" / "stock.rs").write_text("fn stock() {}\n", encoding="utf-8")
            (repo / "README.md").write_text("upstream\n", encoding="utf-8")
            base = commit_all(repo, "upstream")

            (repo / "core" / "stock.rs").write_text("fn stock() {}\nfn hook() {}\n", encoding="utf-8")
            (repo / "core" / "grok.rs").write_text("fn grok() {}\n", encoding="utf-8")
            (repo / "grokex").mkdir()
            (repo / "grokex" / "release.py").write_bytes(b"print(1)\n\x00binary\n")
            (repo / "README.md").unlink()
            head = commit_all(repo, "graft")

            series = Path(scratch) / "series.json"
            series.write_text(
                json.dumps(
                    {
                        "patches": [
                            {"name": "core", "summary": "core seam", "paths": ["core/"]},
                            {"name": "tooling", "summary": "tooling", "paths": ["grokex/"]},
                            {"name": "docs", "summary": "docs", "paths": ["README.md"]},
                        ]
                    }
                ),
                encoding="utf-8",
            )
            out = Path(scratch) / "series"

            patches = seam_series.load_series(series)
            written = seam_series.export(repo, patches, base, head, out)
            tree = seam_series.verify(repo, base, head, out)

            self.assertEqual(
                [path.name for path in written],
                ["0001-core.patch", "0002-tooling.patch", "0003-docs.patch"],
            )
            self.assertEqual(tree, git(repo, "rev-parse", f"{head}^{{tree}}"))

            # The CLI takes range options after the subcommand, the way
            # grokex-checks invokes it.
            cli_out = Path(scratch) / "cli-series"
            cli_args = ["--repo", str(repo), "--series", str(series), "--base", base, "--head", head]
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(seam_series.main(["plan", *cli_args]), 0)
                self.assertEqual(seam_series.main(["export", *cli_args, "--out", str(cli_out)]), 0)
                self.assertEqual(seam_series.main(["verify", *cli_args, "--out", str(cli_out)]), 0)
            self.assertEqual(
                sorted(path.name for path in cli_out.glob("*.patch")),
                [path.name for path in written],
            )

            replay = Path(scratch) / "replay"
            git(repo, "worktree", "add", "-q", "--detach", str(replay), base)
            subprocess.run(
                [
                    "git",
                    "-c",
                    "user.name=Seam Test",
                    "-c",
                    "user.email=seam@example.invalid",
                    "am",
                    "-q",
                    *(str(path) for path in written),
                ],
                cwd=replay,
                check=True,
                capture_output=True,
            )
            self.assertEqual(git(replay, "rev-parse", "HEAD^{tree}"), tree)
            self.assertEqual(
                git(replay, "log", "--format=%s", f"{base}..HEAD").splitlines(),
                [
                    "grokex(docs): docs",
                    "grokex(tooling): tooling",
                    "grokex(core): core seam",
                ],
            )
            git(repo, "worktree", "remove", "--force", str(replay))


if __name__ == "__main__":
    unittest.main()
