#!/usr/bin/env python3
"""Path contract for grokex-checks: which deterministic gates a change needs.

The checks workflow has two gate groups:

- ``rust``: formatting, the stock and Grok cargo contracts, and the affected
  library lints. About 25 minutes of cargo on a cold runner.
- ``helpers``: the Go validator, the Python release helpers, the seam series,
  the shipped scripts, and the Grokex workflow files. About one minute.

``RULES`` maps every changed path to the gates it needs; the first matching
rule wins. A path no rule names needs every gate, so an unforeseen file can
only make the run slower, never weaker. The checks workflow itself always
needs every gate because a change to it must prove itself.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

GATES = ("rust", "helpers")
ALL = GATES
NONE: tuple[str, ...] = ()

# (kind, pattern, gates). kind is "exact", "prefix", or "suffix".
RULES: tuple[tuple[str, str, tuple[str, ...]], ...] = (
    ("exact", ".github/workflows/grokex-checks.yml", ALL),
    ("prefix", ".github/workflows/grokex-", ("helpers",)),
    # Stock workflows are disabled on the fork; build scripts only feed
    # grokex-build, whose push run is their check.
    ("prefix", ".github/workflows/", NONE),
    ("prefix", ".github/scripts/", NONE),
    # setup-ci and the toolchain actions feed every cargo job.
    ("prefix", ".github/actions/", ("rust",)),
    ("prefix", ".github/", NONE),
    # Rust sources include Markdown prompts through include_str!, so the
    # codex-rs rule precedes the Markdown rule.
    ("prefix", "codex-rs/", ("rust",)),
    ("prefix", "docs/", NONE),
    ("suffix", ".md", NONE),
    ("prefix", "grokex/", ("helpers",)),
    # LICENSE ships inside every archive; the helpers package it.
    ("exact", "LICENSE", ("helpers",)),
)


def gates_for(path: str) -> tuple[str, ...]:
    """Gates one changed path needs; every gate when no rule names it."""
    for kind, pattern, gates in RULES:
        if kind == "exact" and path == pattern:
            return gates
        if kind == "prefix" and path.startswith(pattern):
            return gates
        if kind == "suffix" and path.endswith(pattern):
            return gates
    return ALL


def scope(paths: list[str]) -> dict[str, bool]:
    """Gate map for a change; an empty change needs every gate."""
    if not paths:
        return {gate: True for gate in GATES}
    needed = {gate: False for gate in GATES}
    for path in paths:
        for gate in gates_for(path):
            needed[gate] = True
    return needed


def changed_paths(repository: Path, base: str, head: str) -> list[str]:
    """Paths whose content differs between the two commits' trees.

    A two-dot tree comparison is used on purpose: on a shallow checkout there
    is no merge base, and counting files the base branch changed since the
    fork point only adds gates.
    """
    output = subprocess.run(
        ["git", "diff", "--name-only", "--no-renames", base, head],
        cwd=repository,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return [line for line in output.splitlines() if line]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--repository", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--base", help="commit the change is compared against")
    parser.add_argument("--head", default="HEAD")
    parser.add_argument("--all", action="store_true", help="require every gate without diffing")
    parser.add_argument("--github-output", action="store_true")
    args = parser.parse_args()

    if args.all:
        paths: list[str] = []
        needed = scope(paths)
    else:
        if not args.base:
            raise SystemExit("--base is required unless --all is given")
        paths = changed_paths(args.repository, args.base, args.head)
        needed = scope(paths)
        for path in paths:
            gates = ",".join(gates_for(path)) or "-"
            print(f"{gates:>13}  {path}", file=sys.stderr)
    for gate in GATES:
        value = "true" if needed[gate] else "false"
        print(f"{gate}={value}" if args.github_output else f"{gate}: {value}")


if __name__ == "__main__":
    main()
