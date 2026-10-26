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
from select_artifact import (REPOSITORY, SELECTION_FAILURE_REASONS, SelectionFailure,
                             linked_pull_request_number, read_metadata, select)


MAX_METADATA_BYTES = 16 * 1024 * 1024
MAX_GH_STDERR_BYTES = 16 * 1024
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
    "pull_request_request", "pull_request_metadata",
})


# These are gh-reported classifications, not proof of an underlying cause.
# Output is selected from constants, never redacted or copied from stderr.
GH_HTTP_SUMMARIES = {
    b"400": "gh=bad_request; http=400",
    b"401": "gh=authentication; http=401",
    b"403": "gh=permission_denied; http=403",
    b"404": "gh=not_found; http=404",
    b"408": "gh=timeout; http=408",
    b"409": "gh=conflict; http=409",
    b"410": "gh=gone; http=410",
    b"422": "gh=validation; http=422",
    b"429": "gh=rate_limit; http=429",
    b"500": "gh=server_error; http=500",
    b"502": "gh=server_error; http=502",
    b"503": "gh=server_error; http=503",
    b"504": "gh=server_error; http=504",
}
GH_ERROR_SUMMARIES = frozenset(GH_HTTP_SUMMARIES.values()) | {
    "gh=rate_limit; http=403", "gh=authentication", "gh=network",
}


def safe_gh_summary(value: object) -> str | None:
    return value if type(value) is str and value in GH_ERROR_SUMMARIES else None


def summarize_gh_stderr(stderr: bytes) -> str | None:
    """Recognize bounded, printable gh diagnostics; never return their text."""
    if len(stderr) > MAX_GH_STDERR_BYTES:
        return None
    summaries = set()
    for line in stderr.split(b"\n"):
        line = line.removesuffix(b"\r")
        match = re.fullmatch(rb"gh: [ -~]* \(HTTP ([0-9]{3})\)", line)
        if match is None:
            match = re.fullmatch(rb"HTTP ([0-9]{3}): [ -~]*", line)
        if match is not None:
            status = match[1]
            summary = GH_HTTP_SUMMARIES.get(status)
            if summary is None:
                return None
            if status == b"403" and line.startswith((b"gh: API rate limit exceeded for ",
                                                     b"gh: You have exceeded a secondary rate limit")):
                summary = "gh=rate_limit; http=403"
            summaries.add(summary)
        elif line == b"To get started with GitHub CLI, please run:":
            summaries.add("gh=authentication")
        elif line in (b"error connecting to api.github.com", b"error connecting to github.com"):
            summaries.add("gh=network")
    return summaries.pop() if len(summaries) == 1 else None


class GhApiFailure(ValueError):
    def __init__(self, summary: str | None):
        self.gh_summary = safe_gh_summary(summary)
        super().__init__("GitHub API request failed")


def safe_selection_reason(value: object) -> str | None:
    return value if type(value) is str and value in SELECTION_FAILURE_REASONS else None


class AcquisitionFailure(ValueError):
    def __init__(self, code: str, gh_summary: str | None = None, *, selection_reason: str | None = None):
        self.code = code if type(code) is str and code in FAILURE_CODES else "unknown"
        self.gh_summary = safe_gh_summary(gh_summary)
        self.selection_reason = safe_selection_reason(selection_reason) if self.code == "selection" else None
        super().__init__(self.code)


@contextmanager
def acquisition_stage(code: str):
    try:
        yield
    except AcquisitionFailure:
        raise
    except SelectionFailure as error:
        raise AcquisitionFailure(code, selection_reason=getattr(error, "reason", None)) from None
    except ACQUISITION_ERRORS as error:
        summary = error.gh_summary if isinstance(error, GhApiFailure) else None
        raise AcquisitionFailure(code, summary) from None


def gh_api(endpoint: str, destination: Path, byte_limit: int) -> None:
    """Keep API output private and bounded, including unsuccessful responses."""
    deadline = time.monotonic() + GH_TIMEOUT_SECONDS
    with destination.open("xb") as target, subprocess.Popen(
        ["gh", "api", "--hostname", "github.com", "--method", "GET", endpoint],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    ) as process:
        try:
            with selectors.DefaultSelector() as ready:
                ready.register(process.stdout, selectors.EVENT_READ, "stdout")
                ready.register(process.stderr, selectors.EVENT_READ, "stderr")
                size = 0
                stderr = bytearray()
                stderr_truncated = False
                while ready.get_map():
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise TimeoutError("GitHub API request timed out")
                    events = ready.select(remaining)
                    if not events:
                        raise TimeoutError("GitHub API request timed out")
                    for key, _ in events:
                        block = os.read(key.fd, 64 * 1024)
                        if not block:
                            ready.unregister(key.fileobj)
                        elif key.data == "stderr":
                            # Drain both pipes under the same deadline. Excess stderr
                            # is discarded, never spooled or used as a partial summary.
                            if not stderr_truncated:
                                if len(stderr) + len(block) > MAX_GH_STDERR_BYTES:
                                    stderr.clear()
                                    stderr_truncated = True
                                else:
                                    stderr.extend(block)
                        else:
                            size += len(block)
                            if size > byte_limit:
                                raise ValueError("GitHub API response exceeds its byte budget")
                            target.write(block)
            if process.wait(timeout=max(0, deadline - time.monotonic())) != 0:
                summary = None if stderr_truncated else summarize_gh_stderr(stderr)
                raise GhApiFailure(summary)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()


def fetch(run_id: int, harness_sha: str, output: Path, *,
          same_run_source_sha: str | None = None,
          expected_artifact_id: int | None = None,
          expected_run_attempt: int | None = None,
          pr_diagnostic: bool = False) -> dict:
    with acquisition_stage("input"):
        if type(run_id) is not int or run_id <= 0 or not re.fullmatch(r"[0-9a-f]{40}", harness_sha):
            raise ValueError("invalid explicit run or harness revision")
        same_run = any(value is not None for value in (
            same_run_source_sha, expected_artifact_id, expected_run_attempt))
        if type(pr_diagnostic) is not bool or (pr_diagnostic and same_run):
            raise ValueError("PR diagnostics cannot use same-run selection")
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
        pull_request = None
        if pr_diagnostic:
            with acquisition_stage("selection"):
                number = linked_pull_request_number(run)
            with acquisition_stage("pull_request_request"):
                gh_api(f"/repos/{REPOSITORY}/pulls/{number}", root / "pull-request.json", MAX_METADATA_BYTES)
            with acquisition_stage("pull_request_metadata"):
                pull_request = read_metadata(root / "pull-request.json")
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
                              expected_run_attempt=expected_run_attempt,
                              pull_request=pull_request)
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
    parser.add_argument("--pr-diagnostic", action="store_true")
    parser.add_argument("--expected-artifact-id", type=int)
    parser.add_argument("--expected-run-attempt", type=int)
    args = parser.parse_args()
    try:
        result = fetch(args.run_id, args.harness_sha, args.output,
                       same_run_source_sha=args.same_run_source_sha,
                       expected_artifact_id=args.expected_artifact_id,
                       expected_run_attempt=args.expected_run_attempt,
                       pr_diagnostic=args.pr_diagnostic)
    except ACQUISITION_ERRORS as error:
        code = error.code if isinstance(error, AcquisitionFailure) else "unknown"
        if type(code) is not str or code not in FAILURE_CODES:
            code = "unknown"
        summary = safe_gh_summary(error.gh_summary) if isinstance(error, AcquisitionFailure) else None
        detail = f"; {summary}" if summary else ""
        if isinstance(error, AcquisitionFailure) and code == "selection":
            reason = safe_selection_reason(getattr(error, "selection_reason", None))
            if reason:
                detail += f"; selection={reason}"
        print(f"Package artifact acquisition failed [{code}]{detail}; no package execution was attempted.",
              file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

