#!/usr/bin/env python3
"""Validate one explicit immutable Linux build artifact from API facts."""

import argparse
import json
from pathlib import Path
import re
import sys
import subprocess


REPOSITORY = "Harness-X-Harness/codex"
TARGET = "x86_64-unknown-linux-musl"


def producer_names(target: str) -> frozenset[str]:
    # Published artifacts keep their original identity after workflow extraction.
    return frozenset({"Build " + target, "Build " + target + " / Package"})


PRODUCER_NAMES = producer_names(TARGET)
SELECTION_FAILURE_REASONS = frozenset(
    {
        "run_state",
        "jobs_inventory",
        "identity",
        "producer",
        "artifact",
    }
)


class SelectionFailure(ValueError):
    """A rejected selection with a fixed, safe diagnostic reason."""

    def __init__(self, reason: str, message: str):
        if type(reason) is not str or reason not in SELECTION_FAILURE_REASONS:
            raise ValueError("invalid selection failure reason")
        self.reason = reason
        super().__init__(message)


# Binary inputs are the default; these paths are owned by consumers only.
CONSUMER_DIRECTORIES = tuple(
    "grok/" + name + "/"
    for name in (
        "live",
        "tests",
        "facts",
        "cmd",
        "internal",
        "docs",
    )
)
CONSUMER_FILES = frozenset(
    {
        "README.md",
        "CHANGELOG.md",
        "grok/go.mod",
        "grok/go.sum",
        "grok/TESTING.md",
        "grok/package/fetch_artifact.py",
        "grok/package/select_artifact.py",
        "grok/package/extract_artifact.py",
        "grok/package/live_diagnostics.py",
        "grok/package/build_selection.py",
        "grok/package/verify_provider.sh",
        "grok/package/README.md",
        "grok/package/ARTIFACTS.md",
        ".github/workflows/grok.yml",
        ".github/workflows/grok-live.yml",
        ".github/workflows/grok-facts.yml",
        ".github/workflows/grok-go.yml",
        ".github/workflows/grok-proof.yml",
    }
)


def is_binary_input(path: str) -> bool:
    if path in CONSUMER_FILES or path.startswith(CONSUMER_DIRECTORIES):
        return False
    if (
        path.startswith("grok/package/test_")
        and path.endswith(".py")
        and "/" not in path[len("grok/package/") :]
    ):
        return False
    return True


def ensure_source_commit(checkout: Path, source: str) -> None:
    """Materialize only the authenticated commit from the fixed public repository."""
    if not re.fullmatch(r"[0-9a-f]{40}", source):
        raise ValueError("invalid source commit")
    command = ["git", "cat-file", "-e", source + "^{commit}"]
    result = subprocess.run(
        command,
        cwd=checkout,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        timeout=30,
    )
    if result.returncode != 0:
        subprocess.run(
            [
                "git",
                "fetch",
                "--no-tags",
                "https://github.com/" + REPOSITORY + ".git",
                source,
            ],
            cwd=checkout,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=30,
            check=True,
        )
        subprocess.run(
            command,
            cwd=checkout,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=30,
            check=True,
        )


def binary_inputs_changed(checkout: Path, source: str, subject: str) -> bool:
    ensure_source_commit(checkout, source)
    # No rename detection: both old and new paths matter, including deletes.
    result = subprocess.run(
        ["git", "diff", "--name-only", "--no-renames", "-z", source, subject, "--"],
        cwd=checkout,
        capture_output=True,
        timeout=30,
        check=True,
    )
    paths = result.stdout.decode("utf-8", errors="strict").split("\0")
    return any(is_binary_input(path) for path in paths if path)


def artifact_identity(
    artifact: dict, artifact_id: int, target: str = TARGET
) -> tuple[int, int, str]:
    """Read producer identity from this exact immutable artifact, never a current attempt."""
    if (
        target not in (TARGET, "aarch64-apple-darwin")
        or type(artifact_id) is not int
        or artifact_id <= 0
        or artifact.get("id") != artifact_id
    ):
        raise SelectionFailure(
            "artifact", "artifact identity does not match the explicit selection"
        )
    match = re.fullmatch(
        r"grok-([0-9a-f]{40})-" + re.escape(target) + r"-([1-9][0-9]*)-([1-9][0-9]*)",
        artifact.get("name", ""),
    )
    identity = artifact.get("workflow_run", {})
    if match is None or identity.get("id") != int(match[2]):
        raise SelectionFailure(
            "artifact", "artifact producer identity is missing or inconsistent"
        )
    return int(match[2]), int(match[3]), match[1]


def select(
    run: dict,
    jobs: dict,
    artifact: dict,
    artifact_id: int,
    harness_sha: str,
    *,
    expected_line: str | None = None,
    producer_source: dict | None = None,
    target: str = TARGET,
) -> dict:
    """Validate the actual producer independently of the current consumer harness."""
    if not re.fullmatch(r"[0-9a-f]{40}", harness_sha) or (
        expected_line is not None
        and not re.fullmatch(r"grok/rust-v[^\s]+", expected_line)
    ):
        raise SelectionFailure("identity", "invalid harness or producer line")
    run_id, attempt, artifact_source = artifact_identity(artifact, artifact_id, target)
    sha = run.get("head_sha", "")
    if (
        run.get("id") != run_id
        or run.get("event") not in ("push", "pull_request")
        or run.get("path") != ".github/workflows/grok.yml"
        or not re.fullmatch(r"[0-9a-f]{40}", sha)
        or run.get("repository", {}).get("full_name") != REPOSITORY
        or run.get("head_repository", {}).get("full_name") != REPOSITORY
    ):
        raise SelectionFailure("identity", "inappropriate package producer run")
    if run["event"] == "push":
        source_sha, line = sha, run.get("head_branch", "")
    else:
        if (
            producer_source is None
            or producer_source.get("head_sha") != sha
            or producer_source.get("run_id") != run_id
        ):
            raise SelectionFailure(
                "identity", "PR producer checkout is not authenticated"
            )
        source_sha, line = (
            producer_source.get("source_sha"),
            producer_source.get("line", ""),
        )
    if (
        not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._/-]*", line)
        or artifact_source != source_sha
        or (expected_line is not None and line != expected_line)
    ):
        raise SelectionFailure(
            "identity", "artifact source or candidate line does not match"
        )
    job_list = jobs.get("jobs")
    if not isinstance(job_list, list) or jobs.get("total_count") != len(job_list):
        raise SelectionFailure(
            "jobs_inventory", "complete producer job inventory is required"
        )
    producers = [job for job in job_list if job.get("name") in producer_names(target)]
    if len(producers) != 1:
        raise SelectionFailure("producer", "missing or ambiguous package producer")
    producer = producers[0]
    if (
        type(producer.get("id")) is not int
        or producer["id"] <= 0
        or producer.get("run_id") != run_id
        or producer.get("run_attempt") != attempt
        or producer.get("status") != "completed"
        or producer.get("conclusion") != "success"
    ):
        raise SelectionFailure(
            "producer", "the artifact's original package build did not succeed"
        )
    identity = artifact.get("workflow_run", {})
    digest = artifact.get("digest", "")
    if (
        artifact.get("expired") is not False
        or type(artifact.get("size_in_bytes")) is not int
        or artifact["size_in_bytes"] <= 0
        or identity.get("head_sha") != sha
        or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest)
    ):
        raise SelectionFailure(
            "artifact", "unavailable or misidentified package artifact"
        )
    selected = {
        "repository": REPOSITORY,
        "run_id": run_id,
        "run_attempt": attempt,
        "source_sha": source_sha,
        "producer_line": line,
        "target": target,
        "harness_sha": harness_sha,
        "artifact_id": artifact_id,
        "artifact_name": artifact["name"],
        "artifact_digest": digest,
        "artifact_size": artifact["size_in_bytes"],
        "producer_job_id": producer["id"],
        "selection_mode": "line_bound" if expected_line is not None else "diagnostic",
        # Observation only: neither sibling jobs nor later attempts own this package.
        "run_status": run.get("status")
        if run.get("status")
        in ("queued", "in_progress", "completed", "waiting", "requested", "pending")
        else "unknown",
        "run_conclusion": run.get("conclusion")
        if run.get("conclusion")
        in (
            None,
            "success",
            "failure",
            "cancelled",
            "timed_out",
            "action_required",
            "neutral",
            "skipped",
            "stale",
            "startup_failure",
        )
        else "unknown",
    }
    if producer_source is not None:
        selected.update(producer_source=producer_source)
    return selected


def linked_pull_request_number(run: dict) -> int:
    links = run.get("pull_requests")
    if (
        run.get("event") != "pull_request"
        or not isinstance(links, list)
        or len(links) != 1
        or not isinstance(links[0], dict)
        or type(links[0].get("number")) is not int
        or links[0]["number"] <= 0
    ):
        raise SelectionFailure(
            "identity", "one explicit same-repository PR is required"
        )
    return links[0]["number"]


def pr_source(run: dict, pull_request: dict, harness_sha: str) -> str:
    """Bind PR head/base metadata to the current merge source, not another PR."""
    number = linked_pull_request_number(run)
    try:
        linked = run["pull_requests"][0]
        head, base = pull_request["head"], pull_request["base"]
        source = pull_request["merge_commit_sha"]
        valid = (
            pull_request.get("number") == number
            and pull_request.get("state") == "open"
            and pull_request.get("merged") is False
            and head["repo"]["full_name"] == base["repo"]["full_name"] == REPOSITORY
            and head["sha"] == harness_sha == run["head_sha"]
            and head["ref"] == run["head_branch"]
            and base["ref"].startswith("grok/rust-v")
            and re.fullmatch(r"[0-9a-f]{40}", base["sha"])
            and re.fullmatch(r"[0-9a-f]{40}", source)
            and all(
                linked[side][field] == pull_request[side][field]
                for side in ("head", "base")
                for field in ("sha", "ref")
            )
            and all(
                linked[side]["repo"]["id"] == pull_request[side]["repo"]["id"]
                for side in ("head", "base")
            )
        )
    except (KeyError, TypeError, AttributeError):
        valid = False
    if not valid:
        raise SelectionFailure(
            "identity", "PR package does not match the current diagnostic subject"
        )
    return source


def admit_harness(
    pull_request: dict, ci_run: dict, harness_sha: str, harness_ref: str
) -> str:
    """Admit the current PR harness independently of the selected binary producer."""
    if (
        ci_run.get("status") != "completed"
        or ci_run.get("conclusion") != "success"
        or ci_run.get("path") != ".github/workflows/grok.yml"
        or ci_run.get("repository", {}).get("full_name") != REPOSITORY
        or ci_run.get("head_repository", {}).get("full_name") != REPOSITORY
    ):
        raise SelectionFailure("run_state", "PR harness requires successful current CI")
    if pull_request.get("head", {}).get("ref") != harness_ref:
        raise SelectionFailure("identity", "PR harness ref does not match")
    return pr_source(ci_run, pull_request, harness_sha)


def read_metadata(path: Path) -> dict:
    if path.stat().st_size > 16 * 1024 * 1024:
        raise ValueError("API metadata exceeds the bounded inventory size")
    with path.open(encoding="utf-8") as source:
        value = json.load(source)
    if not isinstance(value, dict):
        raise ValueError("API metadata must be an object")
    return value


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact-id", type=int, required=True)
    parser.add_argument("--harness-sha", required=True)
    parser.add_argument("--expected-line")
    parser.add_argument("--producer-source-json", type=Path)
    for name in ("run", "jobs", "artifact"):
        parser.add_argument("--" + name + "-json", type=Path, required=True)
    args = parser.parse_args()
    try:
        selected = select(
            read_metadata(args.run_json),
            read_metadata(args.jobs_json),
            read_metadata(args.artifact_json),
            args.artifact_id,
            args.harness_sha,
            expected_line=args.expected_line,
            producer_source=read_metadata(args.producer_source_json)
            if args.producer_source_json
            else None,
        )
    except (OSError, ValueError, KeyError, TypeError, AttributeError):
        print("No eligible explicit package artifact was selected.", file=sys.stderr)
        return 1
    print(json.dumps(selected, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
