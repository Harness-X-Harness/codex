#!/usr/bin/env python3
"""Validate one explicit immutable Linux build artifact from API facts."""

import argparse
import json
from pathlib import Path
import re
import sys


REPOSITORY = "Harness-X-Harness/codex"
TARGET = "x86_64-unknown-linux-musl"
SELECTION_FAILURE_REASONS = frozenset({
    "run_state", "jobs_inventory", "identity", "producer", "artifact",
})


class SelectionFailure(ValueError):
    """A rejected selection with a fixed, safe diagnostic reason."""

    def __init__(self, reason: str, message: str):
        if type(reason) is not str or reason not in SELECTION_FAILURE_REASONS:
            raise ValueError("invalid selection failure reason")
        self.reason = reason
        super().__init__(message)


def artifact_identity(artifact: dict, artifact_id: int) -> tuple[int, int, str]:
    """Read producer identity from this exact immutable artifact, never a current attempt."""
    if (type(artifact_id) is not int or artifact_id <= 0
            or artifact.get("id") != artifact_id):
        raise SelectionFailure("artifact", "artifact identity does not match the explicit selection")
    match = re.fullmatch(r"grok-([0-9a-f]{40})-" + re.escape(TARGET) + r"-([1-9][0-9]*)-([1-9][0-9]*)",
                         artifact.get("name", ""))
    identity = artifact.get("workflow_run", {})
    if match is None or identity.get("id") != int(match[2]):
        raise SelectionFailure("artifact", "artifact producer identity is missing or inconsistent")
    return int(match[2]), int(match[3]), match[1]


def select(run: dict, jobs: dict, artifact: dict, artifact_id: int, harness_sha: str,
           *, expected_source_sha: str | None = None,
           pull_request: dict | None = None) -> dict:
    """Validate one artifact and its original producer; no fallback or execution."""
    if (not re.fullmatch(r"[0-9a-f]{40}", harness_sha)
            or (expected_source_sha is not None and expected_source_sha != harness_sha)
            or (expected_source_sha is not None and pull_request is not None)):
        raise SelectionFailure("identity", "invalid harness or formal source revision")
    run_id, attempt, artifact_source = artifact_identity(artifact, artifact_id)
    sha = run.get("head_sha", "")
    if (
        run.get("id") != run_id
        or run.get("event") != ("pull_request" if pull_request is not None else "push")
        or run.get("path") != ".github/workflows/grok.yml"
        or (pull_request is None and not run.get("head_branch", "").startswith("grok/rust-v"))
        or not re.fullmatch(r"[0-9a-f]{40}", sha)
        or run.get("repository", {}).get("full_name") != REPOSITORY
        or run.get("head_repository", {}).get("full_name") != REPOSITORY
    ):
        raise SelectionFailure("identity", "inappropriate package producer run")
    source_sha = pr_source(run, pull_request, harness_sha) if pull_request is not None else sha
    if artifact_source != source_sha or (expected_source_sha is not None and source_sha != expected_source_sha):
        raise SelectionFailure("identity", "artifact source differs from the consuming subject")
    job_list = jobs.get("jobs")
    if not isinstance(job_list, list) or jobs.get("total_count") != len(job_list):
        raise SelectionFailure("jobs_inventory", "complete producer job inventory is required")
    producers = [job for job in job_list if job.get("name") == "Build " + TARGET]
    if len(producers) != 1:
        raise SelectionFailure("producer", "missing or ambiguous package producer")
    producer = producers[0]
    if (
        type(producer.get("id")) is not int or producer["id"] <= 0
        or producer.get("run_id") != run_id or producer.get("run_attempt") != attempt
        or producer.get("status") != "completed" or producer.get("conclusion") != "success"
    ):
        raise SelectionFailure("producer", "the artifact's original package build did not succeed")
    identity = artifact.get("workflow_run", {})
    digest = artifact.get("digest", "")
    if (
        artifact.get("expired") is not False
        or type(artifact.get("size_in_bytes")) is not int or artifact["size_in_bytes"] <= 0
        or identity.get("head_sha") != sha
        or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest)
    ):
        raise SelectionFailure("artifact", "unavailable or misidentified package artifact")
    selected = {
        "repository": REPOSITORY, "run_id": run_id, "run_attempt": attempt,
        "source_sha": source_sha, "target": TARGET, "harness_sha": harness_sha,
        "artifact_id": artifact_id, "artifact_name": artifact["name"],
        "artifact_digest": digest, "artifact_size": artifact["size_in_bytes"],
        "producer_job_id": producer["id"],
        "selection_mode": "pr_diagnostic" if pull_request is not None else (
            "formal" if expected_source_sha is not None else "diagnostic"),
        # Observation only: neither sibling jobs nor later attempts own this package.
        "run_status": run.get("status") if run.get("status") in (
            "queued", "in_progress", "completed", "waiting", "requested", "pending"
        ) else "unknown",
        "run_conclusion": run.get("conclusion") if run.get("conclusion") in (
            None, "success", "failure", "cancelled", "timed_out", "action_required",
            "neutral", "skipped", "stale", "startup_failure",
        ) else "unknown",
    }
    if pull_request is not None:
        selected.update(pull_request_number=pull_request["number"], pr_head_sha=sha)
    return selected


def linked_pull_request_number(run: dict) -> int:
    links = run.get("pull_requests")
    if (run.get("event") != "pull_request" or not isinstance(links, list) or len(links) != 1
            or not isinstance(links[0], dict) or type(links[0].get("number")) is not int
            or links[0]["number"] <= 0):
        raise SelectionFailure("identity", "one explicit same-repository PR is required")
    return links[0]["number"]


def pr_source(run: dict, pull_request: dict, harness_sha: str) -> str:
    """Bind the current PR head to its tested merge-source package, not another run."""
    if run.get("status") != "completed" or run.get("conclusion") != "success":
        raise SelectionFailure("run_state", "PR diagnostic requires successful settled CI")
    number = linked_pull_request_number(run)
    try:
        linked = run["pull_requests"][0]
        head, base = pull_request["head"], pull_request["base"]
        source = pull_request["merge_commit_sha"]
        valid = (
            pull_request.get("number") == number and pull_request.get("state") == "open"
            and pull_request.get("merged") is False
            and head["repo"]["full_name"] == base["repo"]["full_name"] == REPOSITORY
            and head["sha"] == harness_sha == run["head_sha"]
            and head["ref"] == run["head_branch"]
            and base["ref"].startswith("grok/rust-v")
            and re.fullmatch(r"[0-9a-f]{40}", base["sha"])
            and re.fullmatch(r"[0-9a-f]{40}", source)
            and all(linked[side][field] == pull_request[side][field]
                    for side in ("head", "base") for field in ("sha", "ref"))
            and all(linked[side]["repo"]["id"] == pull_request[side]["repo"]["id"]
                    for side in ("head", "base"))
        )
    except (KeyError, TypeError, AttributeError):
        valid = False
    if not valid:
        raise SelectionFailure("identity", "PR package does not match the current diagnostic subject")
    return source


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
    parser.add_argument("--expected-source-sha")
    parser.add_argument("--pull-request-json", type=Path)
    for name in ("run", "jobs", "artifact"):
        parser.add_argument("--" + name + "-json", type=Path, required=True)
    args = parser.parse_args()
    try:
        selected = select(read_metadata(args.run_json), read_metadata(args.jobs_json),
                          read_metadata(args.artifact_json), args.artifact_id, args.harness_sha,
                          expected_source_sha=args.expected_source_sha,
                          pull_request=read_metadata(args.pull_request_json) if args.pull_request_json else None)
    except (OSError, ValueError, KeyError, TypeError, AttributeError):
        print("No eligible explicit package artifact was selected.", file=sys.stderr)
        return 1
    print(json.dumps(selected, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

