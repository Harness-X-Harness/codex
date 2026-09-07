#!/usr/bin/env python3
"""Prove a harness/<stable-tag> line has the matching stock tag as ancestor.

The check derives the tag from the target branch name. It does not hard-code a
Codex version and is not a second source-authority.
"""

import argparse
import subprocess
import sys
from collections.abc import Sequence
from pathlib import Path


class ProvenanceError(Exception):
    """Branch/tag mapping or ancestry failed."""


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


def git(repo: Path, *args: str) -> str:
    return subprocess.check_output(
        ["git", *args],
        cwd=repo,
        stderr=subprocess.PIPE,
        text=True,
    ).strip()


def short_sha(repo: Path, ref: str) -> str:
    try:
        return git(repo, "rev-parse", "--verify", "--short", f"{ref}^{{commit}}")
    except subprocess.CalledProcessError as error:
        detail = error.stderr.strip() or error.stdout.strip() or str(error)
        raise ProvenanceError(f"missing ref {ref}: {detail}") from error


def is_ancestor(repo: Path, ancestor: str, descendant: str) -> bool:
    result = subprocess.run(
        ["git", "merge-base", "--is-ancestor", ancestor, descendant],
        cwd=repo,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        return True
    if result.returncode == 1:
        return False
    detail = result.stderr.strip() or result.stdout.strip() or str(result.returncode)
    raise ProvenanceError(f"ancestry check failed for {ancestor} -> {descendant}: {detail}")


def check_provenance(repo: Path, base_branch: str, lineage_refs: Sequence[str]) -> str:
    tag = expected_stock_tag(base_branch)
    try:
        tag_sha = short_sha(repo, tag)
    except ProvenanceError as error:
        raise ProvenanceError(f"missing stock tag {tag}: {error}") from error

    lines = [
        f"harness_branch={base_branch}",
        f"stock_tag={tag}",
        f"tag_sha={tag_sha}",
    ]
    if not lineage_refs:
        raise ProvenanceError("no lineage refs to check")
    for ref in lineage_refs:
        sha = short_sha(repo, ref)
        if not is_ancestor(repo, tag, ref):
            raise ProvenanceError(
                f"stock tag {tag} ({tag_sha}) is not an ancestor of {ref} ({sha})"
            )
        lines.append(f"lineage={ref}:{sha} ancestor=yes")
    return "\n".join(lines)


def parse_args(argv=None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-branch", required=True)
    parser.add_argument("--lineage-ref", action="append", default=[])
    parser.add_argument("--repo", default=".")
    parser.add_argument(
        "--print-tag",
        action="store_true",
        help="print the derived stock tag and exit",
    )
    return parser.parse_args(argv)


def main(argv=None) -> int:
    args = parse_args(argv)
    try:
        if args.print_tag:
            print(expected_stock_tag(args.base_branch))
            return 0
        report = check_provenance(
            Path(args.repo).resolve(),
            args.base_branch,
            args.lineage_ref,
        )
    except ProvenanceError as error:
        print(f"harness provenance: {error}", file=sys.stderr)
        return 1
    print(report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
