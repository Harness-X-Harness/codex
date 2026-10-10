#!/usr/bin/env python3
"""Select one explicit run/attempt's completed Linux build artifact from API facts."""

import argparse
import json
from pathlib import Path
import re
import sys


REPOSITORY = "Harness-X-Harness/codex"
TARGET = "x86_64-unknown-linux-musl"


def select(run: dict, jobs: dict, artifacts: dict, run_id: int, harness_sha: str,
           *, same_run_source_sha: str | None = None,
           expected_artifact_id: int | None = None,
           expected_run_attempt: int | None = None) -> dict:
    """No API calls, fallback selection, download or execution happens here."""
    if not re.fullmatch(r"[0-9a-f]{40}", harness_sha):
        raise ValueError("invalid harness revision")
    sha = run.get("head_sha", "")
    attempt = run.get("run_attempt")
    same_run = any(value is not None for value in (same_run_source_sha, expected_artifact_id, expected_run_attempt))
    if same_run:
        if (same_run_source_sha != harness_sha or sha != same_run_source_sha
                or type(expected_artifact_id) is not int or expected_artifact_id <= 0
                or type(expected_run_attempt) is not int or expected_run_attempt <= 0
                or attempt != expected_run_attempt
                or run.get("status") != "in_progress" or run.get("conclusion") is not None):
            raise ValueError("same-run package identity does not match the consuming workflow")
    elif run.get("status") != "completed" or run.get("conclusion") not in (
        "success", "failure", "cancelled", "timed_out", "action_required",
        "neutral", "skipped", "stale", "startup_failure",
    ):
        raise ValueError("diagnostic package run has not settled")
    if (
        type(run_id) is not int or run_id <= 0 or run.get("id") != run_id
        or type(attempt) is not int or attempt <= 0
        or run.get("event") != "push"
        or run.get("path") != ".github/workflows/grok.yml"
        or not run.get("head_branch", "").startswith("grok/rust-v")
        or not re.fullmatch(r"[0-9a-f]{40}", sha)
        or run.get("repository", {}).get("full_name") != REPOSITORY
        or run.get("head_repository", {}).get("full_name") != REPOSITORY
    ):
        raise ValueError("inappropriate or unsettled package run")
    job_list = jobs.get("jobs")
    artifact_list = artifacts.get("artifacts")
    if (
        not isinstance(job_list, list) or jobs.get("total_count") != len(job_list)
        or not isinstance(artifact_list, list) or artifacts.get("total_count") != len(artifact_list)
    ):
        raise ValueError("complete job and artifact inventories are required")
    producers = [job for job in job_list if job.get("name") == "Build " + TARGET]
    if len(producers) != 1:
        raise ValueError("missing or ambiguous package producer")
    producer = producers[0]
    if (
        type(producer.get("id")) is not int or producer["id"] <= 0
        or producer.get("run_id") != run_id or producer.get("run_attempt") != attempt
        or producer.get("status") != "completed" or producer.get("conclusion") != "success"
    ):
        raise ValueError("the selected attempt's package build did not succeed")
    name = f"grok-{sha}-{TARGET}-{run_id}-{attempt}"
    matches = [artifact for artifact in artifact_list if artifact.get("name") == name]
    if len(matches) != 1:
        raise ValueError("missing or ambiguous package artifact")
    artifact = matches[0]
    identity = artifact.get("workflow_run", {})
    digest = artifact.get("digest", "")
    if (
        artifact.get("expired") is not False
        or type(artifact.get("id")) is not int or artifact["id"] <= 0
        or type(artifact.get("size_in_bytes")) is not int or artifact["size_in_bytes"] <= 0
        or (same_run and artifact["id"] != expected_artifact_id)
        or identity.get("id") != run_id or identity.get("head_sha") != sha
        or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest)
    ):
        raise ValueError("unavailable or misidentified package artifact")
    return {
        "repository": REPOSITORY, "run_id": run_id, "run_attempt": attempt,
        "source_sha": sha, "target": TARGET, "harness_sha": harness_sha,
        "artifact_id": artifact["id"], "artifact_name": name,
        "artifact_digest": digest, "artifact_size": artifact["size_in_bytes"],
        "producer_job_id": producer["id"],
        "selection_mode": "same_run" if same_run else "diagnostic",
        "run_status": run["status"],
        # A failed sibling job or previous Live remains visible. It cannot
        # retroactively negate this completed build or become a Live pass here.
        "run_conclusion": run.get("conclusion"),
    }


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
    parser.add_argument("--run-id", type=int, required=True)
    parser.add_argument("--harness-sha", required=True)
    parser.add_argument("--same-run-source-sha")
    parser.add_argument("--expected-artifact-id", type=int)
    parser.add_argument("--expected-run-attempt", type=int)
    for name in ("run", "jobs", "artifacts"):
        parser.add_argument("--" + name + "-json", type=Path, required=True)
    args = parser.parse_args()
    try:
        selected = select(read_metadata(args.run_json), read_metadata(args.jobs_json),
                          read_metadata(args.artifacts_json), args.run_id, args.harness_sha,
                          same_run_source_sha=args.same_run_source_sha,
                          expected_artifact_id=args.expected_artifact_id,
                          expected_run_attempt=args.expected_run_attempt)
    except (OSError, ValueError, KeyError, TypeError, AttributeError):
        print("No eligible explicit package artifact was selected.", file=sys.stderr)
        return 1
    print(json.dumps(selected, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
