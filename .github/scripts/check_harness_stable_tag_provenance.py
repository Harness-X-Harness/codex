#!/usr/bin/env python3
"""Prove a harness/<stable-tag> line has the matching stock tag as ancestor.

The check derives the tag from the target branch name. It does not hard-code a
Codex version and is not a second source-authority.
"""

import argparse
import subprocess
from collections.abc import Sequence
from pathlib import Path


class ProvenanceError(Exception):
    """Branch/tag mapping failed."""


def expected_stock_tag(base_branch: str) -> str:
    branch = base_branch.strip()
    if branch.startswith("refs/heads/"):
        branch = branch[len("refs/heads/") :]
    if not branch.startswith("harness/"):
        raise ProvenanceError(f"not a harness stable line: {base_branch}")
    tag = branch[len("harness/") :]
    if not tag or "/" in tag or tag.startswith(".") or any(ch.isspace() for ch in tag):
        raise ProvenanceError(f"invalid harness stable-tag branch: {base_branch}")
    return tag


def stock_tag_ref(tag: str) -> str:
    return f"refs/tags/{tag}"


def git(repo: Path, *args: str) -> str:
    return subprocess.check_output(
        ["git", *args],
        cwd=repo,
        stderr=subprocess.PIPE,
        text=True,
    ).strip()


def short_sha(repo: Path, ref: str) -> str | None:
    try:
        return git(repo, "rev-parse", "--verify", "--short", f"{ref}^{{commit}}")
    except subprocess.CalledProcessError:
        return None


def is_ancestor(repo: Path, ancestor: str, descendant: str) -> bool:
    result = subprocess.run(
        ["git", "merge-base", "--is-ancestor", ancestor, descendant],
        cwd=repo,
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return result.returncode == 0


def check_provenance(
    repo: Path, base_branch: str, lineage_refs: Sequence[str]
) -> tuple[str, bool]:
    lines = [f"harness_branch={base_branch}"]
    try:
        tag = expected_stock_tag(base_branch)
    except ProvenanceError:
        lines.append("stock_tag=invalid")
        return "\n".join(lines), False
    lines.append(f"stock_tag={tag}")
    tag_ref = stock_tag_ref(tag)
    tag_sha = short_sha(repo, tag_ref)
    if tag_sha is None:
        lines.append("tag_sha=missing")
        return "\n".join(lines), False
    lines.append(f"tag_sha={tag_sha}")
    if not lineage_refs:
        lines.append("lineage=missing")
        return "\n".join(lines), False
    ok = True
    for ref in lineage_refs:
        sha = short_sha(repo, ref)
        if sha is None:
            lines.append(f"lineage={ref}:missing ancestor=no")
            ok = False
            continue
        ancestor = is_ancestor(repo, tag_ref, ref)
        lines.append(f"lineage={ref}:{sha} ancestor={'yes' if ancestor else 'no'}")
        if not ancestor:
            ok = False
    return "\n".join(lines), ok


def parse_args(argv=None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-branch", required=True)
    parser.add_argument("--lineage-ref", action="append", default=[])
    parser.add_argument("--repo", default=".")
    return parser.parse_args(argv)


def main(argv=None) -> int:
    args = parse_args(argv)
    report, ok = check_provenance(
        Path(args.repo).resolve(),
        args.base_branch,
        args.lineage_ref,
    )
    print(report)
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
