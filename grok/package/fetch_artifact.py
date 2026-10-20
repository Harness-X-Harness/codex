#!/usr/bin/env python3
"""Fetch and verify one explicit Linux Actions package using authenticated gh.

Authentication is inherited by the official gh CLI from the approved environment.
This command never executes the package. Inventories larger than one 100-item
page fail closed; there is no fallback run, artifact, host, or retry.
"""

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import re
import selectors
import subprocess
import sys
import tarfile
import tempfile
import time
import zipfile
import zlib

from extract_artifact import MAX_PACKAGE_BYTES, extract
from select_artifact import REPOSITORY, read_metadata, select


MAX_METADATA_BYTES = 16 * 1024 * 1024
GH_TIMEOUT_SECONDS = 180
ACQUISITION_ERRORS = (
    OSError, ValueError, KeyError, TypeError, AttributeError, RuntimeError,
    subprocess.SubprocessError, tarfile.TarError, zipfile.BadZipFile, zlib.error, EOFError,
)
# Codes describe only the failed boundary, not its cause. Never derive them
# from an exception message, response body, URL, endpoint or credential.
FAILURE_CODES = frozenset({
    "input", "workspace", "run_request", "run_metadata", "jobs_request",
    "artifacts_request", "jobs_metadata", "artifacts_metadata", "selection",
    "archive_request", "archive_verification", "publication", "unknown",
})


class AcquisitionFailure(ValueError):
    def __init__(self, code: str):
        self.code = code if code in FAILURE_CODES else "unknown"
        super().__init__(self.code)


@contextmanager
def acquisition_stage(code: str):
    try:
        yield
    except AcquisitionFailure:
        raise
    except ACQUISITION_ERRORS:
        raise AcquisitionFailure(code) from None


def gh_api(endpoint: str, destination: Path, byte_limit: int) -> None:
    """Keep API output private and bounded, including unsuccessful responses."""
    deadline = time.monotonic() + GH_TIMEOUT_SECONDS
    with destination.open("xb") as target, subprocess.Popen(
        ["gh", "api", "--hostname", "github.com", "--method", "GET", endpoint],
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
    ) as process:
        try:
            with selectors.DefaultSelector() as ready:
                ready.register(process.stdout, selectors.EVENT_READ)
                size = 0
                while True:
                    remaining = deadline - time.monotonic()
                    if remaining <= 0 or not ready.select(remaining):
                        raise TimeoutError("GitHub API request timed out")
                    block = os.read(process.stdout.fileno(), 64 * 1024)
                    if not block:
                        break
                    size += len(block)
                    if size > byte_limit:
                        raise ValueError("GitHub API response exceeds its byte budget")
                    target.write(block)
            if process.wait(timeout=max(0, deadline - time.monotonic())) != 0:
                raise ValueError("GitHub API request failed")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()


def fetch(run_id: int, harness_sha: str, output: Path, *,
          same_run_source_sha: str | None = None,
          expected_artifact_id: int | None = None,
          expected_run_attempt: int | None = None) -> dict:
    with acquisition_stage("input"):
        if type(run_id) is not int or run_id <= 0 or not re.fullmatch(r"[0-9a-f]{40}", harness_sha):
            raise ValueError("invalid explicit run or harness revision")
        same_run = any(value is not None for value in (
            same_run_source_sha, expected_artifact_id, expected_run_attempt))
        if same_run and (
            same_run_source_sha != harness_sha
            or type(expected_artifact_id) is not int or expected_artifact_id <= 0
            or type(expected_run_attempt) is not int or expected_run_attempt <= 0
        ):
            raise ValueError("incomplete same-run identity")
        if output.exists() or output.is_symlink():
            raise FileExistsError("output must be a new directory")
    with acquisition_stage("workspace"), tempfile.TemporaryDirectory(
        prefix=".grok-artifact-", dir=output.parent,
    ) as temporary:
        root = Path(temporary)
        prefix = f"/repos/{REPOSITORY}/actions"
        with acquisition_stage("run_request"):
            gh_api(f"{prefix}/runs/{run_id}", root / "run.json", MAX_METADATA_BYTES)
        with acquisition_stage("run_metadata"):
            run = read_metadata(root / "run.json")
            attempt = run.get("run_attempt")
            if type(attempt) is not int or attempt <= 0:
                raise ValueError("invalid run attempt")
        with acquisition_stage("jobs_request"):
            gh_api(f"{prefix}/runs/{run_id}/attempts/{attempt}/jobs?per_page=100&page=1",
                   root / "jobs.json", MAX_METADATA_BYTES)
        with acquisition_stage("artifacts_request"):
            gh_api(f"{prefix}/runs/{run_id}/artifacts?per_page=100&page=1",
                   root / "artifacts.json", MAX_METADATA_BYTES)
        with acquisition_stage("jobs_metadata"):
            jobs = read_metadata(root / "jobs.json")
        with acquisition_stage("artifacts_metadata"):
            artifacts = read_metadata(root / "artifacts.json")
        with acquisition_stage("selection"):
            selected = select(run, jobs, artifacts, run_id, harness_sha,
                              same_run_source_sha=same_run_source_sha,
                              expected_artifact_id=expected_artifact_id,
                              expected_run_attempt=expected_run_attempt)
            if selected["artifact_size"] > MAX_PACKAGE_BYTES:
                raise ValueError("selected artifact exceeds the package byte budget")
        with acquisition_stage("archive_request"):
            gh_api(f"{prefix}/artifacts/{selected['artifact_id']}/zip",
                   root / "artifact.zip", selected["artifact_size"])
        with acquisition_stage("archive_verification"):
            verified = extract(root / "artifact.zip", selected, root / "package")
        with acquisition_stage("publication"):
            for name, report in (("selection.json", selected), ("verified.json", verified)):
                path = root / name
                with path.open("x", encoding="utf-8") as target:
                    json.dump(report, target, sort_keys=True)
                    target.write("\n")
                path.chmod(0o600)
            # Exclusive creation preserves an existing destination, including one
            # created while gh was running. Raw responses stay in the private temp.
            output.mkdir(mode=0o700)
            for name in ("package", "selection.json", "verified.json"):
                (root / name).rename(output / name)
    return verified


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", type=int, required=True)
    parser.add_argument("--harness-sha", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--same-run-source-sha")
    parser.add_argument("--expected-artifact-id", type=int)
    parser.add_argument("--expected-run-attempt", type=int)
    args = parser.parse_args()
    try:
        result = fetch(args.run_id, args.harness_sha, args.output,
                       same_run_source_sha=args.same_run_source_sha,
                       expected_artifact_id=args.expected_artifact_id,
                       expected_run_attempt=args.expected_run_attempt)
    except ACQUISITION_ERRORS as error:
        code = error.code if isinstance(error, AcquisitionFailure) else "unknown"
        if code not in FAILURE_CODES:
            code = "unknown"
        print(f"Package artifact acquisition failed [{code}]; no package execution was attempted.",
              file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
