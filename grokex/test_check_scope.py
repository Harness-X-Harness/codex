import subprocess
import tempfile
import unittest
from pathlib import Path

from grokex import check_scope


def git(repo: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=repo, check=True, capture_output=True, text=True
    ).stdout.strip()


def commit_all(repo: Path, message: str) -> str:
    git(repo, "add", "-A")
    git(
        repo,
        "-c",
        "user.name=Scope Test",
        "-c",
        "user.email=scope@example.invalid",
        "commit",
        "-q",
        "-m",
        message,
    )
    return git(repo, "rev-parse", "HEAD")


class PathContractTest(unittest.TestCase):
    def test_every_path_maps_to_the_gates_it_feeds(self) -> None:
        cases = {
            ".github/workflows/grokex-checks.yml": ("rust", "helpers"),
            ".github/workflows/grokex-release.yml": ("helpers",),
            ".github/workflows/rust-ci.yml": (),
            ".github/scripts/install-musl-build-tools.sh": (),
            ".github/actions/setup-ci/action.yml": ("rust",),
            ".github/ISSUE_TEMPLATE/bug.yml": (),
            "codex-rs/core/src/lib.rs": ("rust",),
            "codex-rs/core/prompt.md": ("rust",),
            "docs/config.md": (),
            "README.md": (),
            "grokex/RELEASE_PIPELINE.md": (),
            "grokex/release.py": ("helpers",),
            "grokex/validator/internal/oracle/basic.go": ("helpers",),
            "grokex/dist/install-grokex.sh": ("helpers",),
            "LICENSE": ("helpers",),
            "MODULE.bazel": ("rust", "helpers"),
            "sdk/typescript/package.json": ("rust", "helpers"),
        }
        self.assertEqual({path: check_scope.gates_for(path) for path in cases}, cases)

    def test_scope_unions_gates_and_fails_safe_on_an_empty_change(self) -> None:
        self.assertEqual(check_scope.scope([]), {"rust": True, "helpers": True})
        self.assertEqual(
            check_scope.scope(["grokex/RELEASE_PIPELINE.md", "docs/a.md"]),
            {"rust": False, "helpers": False},
        )
        self.assertEqual(
            check_scope.scope([".github/workflows/grokex-build.yml", "grokex/validator/go.mod"]),
            {"rust": False, "helpers": True},
        )
        self.assertEqual(
            check_scope.scope(["codex-rs/core/src/lib.rs", "README.md"]),
            {"rust": True, "helpers": False},
        )
        self.assertEqual(
            check_scope.scope(["codex-rs/core/src/lib.rs", "grokex/release.py"]),
            {"rust": True, "helpers": True},
        )

    def test_changed_paths_compares_trees_and_lists_deletions_by_old_name(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            repo = Path(scratch)
            git(repo, "init", "-q", "-b", "main")
            (repo / "codex-rs").mkdir()
            (repo / "codex-rs" / "lib.rs").write_text("fn main() {}\n", encoding="utf-8")
            (repo / "grokex").mkdir()
            (repo / "grokex" / "old.py").write_text("x = 1\n", encoding="utf-8")
            base = commit_all(repo, "base")

            (repo / "grokex" / "old.py").rename(repo / "grokex" / "new.py")
            (repo / "README.md").write_text("docs\n", encoding="utf-8")
            head = commit_all(repo, "rename and document")

            self.assertEqual(
                check_scope.changed_paths(repo, base, head),
                ["README.md", "grokex/new.py", "grokex/old.py"],
            )
            self.assertEqual(
                check_scope.scope(check_scope.changed_paths(repo, base, head)),
                {"rust": False, "helpers": True},
            )


if __name__ == "__main__":
    unittest.main()
