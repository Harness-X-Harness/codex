#!/usr/bin/env python3
"""Regroup the Grokex graft into one reviewable patch per stock seam.

The release branch carries the Grok graft as a long commit history on top of
an upstream Codex tag. When upstream moves, replaying that history commit by
commit is noisy. This tool rewrites the *net* difference between the upstream
commit and the branch head into an ordered patch series where every patch owns
exactly one seam from ``seam_series.json``:

    python3 grokex/seam_series.py plan
    python3 grokex/seam_series.py export --out /tmp/grokex-series
    python3 grokex/seam_series.py verify --out /tmp/grokex-series

``plan`` prints the seam assignment and fails on any changed path that no seam
owns. ``export`` writes ``git am``-compatible patches. ``verify`` applies the
series onto the upstream tree in a scratch index and requires the resulting
tree hash to equal the branch tree, so the regrouping is provably lossless.
The default base is ``upstream_commit`` from ``release-source.json``.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
from collections import OrderedDict
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parent
SERIES_PATH = HERE / "seam_series.json"
RELEASE_SOURCE_PATH = HERE / "release-source.json"


def git(repo: Path, *args: str, env: dict[str, str] | None = None) -> str:
    return subprocess.run(
        ["git", *args], cwd=repo, env=env, check=True, capture_output=True, text=True
    ).stdout


def git_bytes(repo: Path, *args: str) -> bytes:
    return subprocess.run(
        ["git", *args], cwd=repo, check=True, capture_output=True
    ).stdout


def load_series(path: Path = SERIES_PATH) -> list[dict[str, object]]:
    series = json.loads(path.read_text(encoding="utf-8"))
    patches: list[dict[str, object]] = series["patches"]
    names = [patch["name"] for patch in patches]
    if len(names) != len(set(names)):
        raise SystemExit("seam series patch names must be unique")
    for patch in patches:
        rules: list[str] = patch["paths"]  # type: ignore[assignment]
        if not rules or any(
            not rule or rule.startswith("/") or "\\" in rule for rule in rules
        ):
            raise SystemExit(f"seam {patch['name']} has a malformed path rule")
    return patches


def default_base() -> str:
    source = json.loads(RELEASE_SOURCE_PATH.read_text(encoding="utf-8"))
    return source["upstream_commit"]


def path_owned_by(rule: str, changed_path: str) -> bool:
    """A rule ending in ``/`` or ``-`` is a prefix; anything else is exact."""
    if rule.endswith(("/", "-")):
        return changed_path.startswith(rule)
    return changed_path == rule


def changed_paths(repo: Path, base: str, head: str) -> list[str]:
    output = git(repo, "diff", "--name-only", "-z", base, head)
    return [path for path in output.split("\0") if path]


def assign(
    patches: list[dict[str, object]], paths: list[str]
) -> tuple[OrderedDict[str, list[str]], list[str]]:
    """Assign every changed path to the first seam whose rule owns it."""
    assignment: OrderedDict[str, list[str]] = OrderedDict(
        (str(patch["name"]), []) for patch in patches
    )
    unowned: list[str] = []
    for path in paths:
        for patch in patches:
            rules: list[str] = patch["paths"]  # type: ignore[assignment]
            if any(path_owned_by(rule, path) for rule in rules):
                assignment[str(patch["name"])].append(path)
                break
        else:
            unowned.append(path)
    return assignment, unowned


def plan(repo: Path, patches: list[dict[str, object]], base: str, head: str) -> int:
    assignment, unowned = assign(patches, changed_paths(repo, base, head))
    for index, patch in enumerate(patches, start=1):
        name = str(patch["name"])
        owned = assignment[name]
        print(f"{index:04d} {name} ({len(owned)} paths): {patch['summary']}")
        for path in owned:
            print(f"    {path}")
    if unowned:
        print("unowned paths:", file=sys.stderr)
        for path in unowned:
            print(f"    {path}", file=sys.stderr)
        return 1
    return 0


def export(
    repo: Path, patches: list[dict[str, object]], base: str, head: str, out: Path
) -> list[Path]:
    assignment, unowned = assign(patches, changed_paths(repo, base, head))
    if unowned:
        raise SystemExit(f"unowned paths: {unowned}")
    out.mkdir(parents=True, exist_ok=True)
    for stale in out.glob("*.patch"):
        stale.unlink()
    author_name, author_email, date = (
        git(repo, "log", "-1", "--format=%an%x00%ae%x00%aD", head)
        .rstrip("\n")
        .split("\0")
    )
    total = len(patches)
    written: list[Path] = []
    for index, patch in enumerate(patches, start=1):
        name = str(patch["name"])
        owned = assignment[name]
        if not owned:
            continue
        diff = git_bytes(
            repo, "diff", "--binary", "--full-index", base, head, "--", *owned
        )
        header = (
            f"From {head} Mon Sep 17 00:00:00 2001\n"
            f"From: {author_name} <{author_email}>\n"
            f"Date: {date}\n"
            f"Subject: [PATCH {index}/{total}] grokex({name}): {patch['summary']}\n"
            "\n"
            f"Seam `{name}` from grokex/seam_series.json.\n"
            f"Net change {base[:12]}..{head[:12]} restricted to:\n"
            + "".join(f"- {path}\n" for path in owned)
            + "---\n"
        )
        target = out / f"{index:04d}-{name}.patch"
        target.write_bytes(header.encode("utf-8") + diff)
        written.append(target)
    if not written:
        raise SystemExit(f"{base[:12]}..{head[:12]} changes nothing")
    return written


def verify(repo: Path, base: str, head: str, out: Path) -> str:
    """Apply the exported series onto the base tree and compare tree hashes."""
    patches = sorted(out.glob("*.patch"))
    if not patches:
        raise SystemExit(f"no patches under {out}")
    expected_tree = git(repo, "rev-parse", f"{head}^{{tree}}").strip()
    with tempfile.TemporaryDirectory() as scratch:
        env = dict(os.environ, GIT_INDEX_FILE=str(Path(scratch) / "index"))
        git(repo, "read-tree", f"{base}^{{tree}}", env=env)
        for patch in patches:
            try:
                git(repo, "apply", "--cached", "--binary", str(patch), env=env)
            except subprocess.CalledProcessError as error:
                print(error.stderr, file=sys.stderr)
                raise SystemExit(f"patch does not apply cleanly: {patch.name}")
        actual_tree = git(repo, "write-tree", env=env).strip()
    if actual_tree != expected_tree:
        raise SystemExit(
            f"series tree {actual_tree} differs from {head} tree {expected_tree}"
        )
    return expected_tree


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--repo", type=Path, default=REPO_ROOT)
    parser.add_argument("--series", type=Path, default=SERIES_PATH)
    parser.add_argument("--base", help="upstream commit (default: release-source.json)")
    parser.add_argument("--head", default="HEAD")
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("plan")
    export_parser = subparsers.add_parser("export")
    export_parser.add_argument("--out", type=Path, required=True)
    verify_parser = subparsers.add_parser("verify")
    verify_parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args(argv)

    repo: Path = args.repo
    base = git(
        repo, "rev-parse", "--verify", f"{args.base or default_base()}^{{commit}}"
    ).strip()
    head = git(repo, "rev-parse", "--verify", f"{args.head}^{{commit}}").strip()
    if args.command == "plan":
        return plan(repo, load_series(args.series), base, head)
    if args.command == "export":
        for path in export(repo, load_series(args.series), base, head, args.out):
            print(path)
        return 0
    if args.command == "verify":
        tree = verify(repo, base, head, args.out)
        print(f"{len(list(args.out.glob('*.patch')))} patches reproduce tree {tree}")
        return 0
    raise SystemExit(f"unknown command {args.command}")


if __name__ == "__main__":
    sys.exit(main())
