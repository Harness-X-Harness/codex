#!/usr/bin/env python3
"""Fetch and verify one explicit Linux Actions package using authenticated gh.

Authentication is inherited by the official gh CLI from the approved environment.
This command never executes the package. Inventories larger than one 100-item
page fail closed; there is no fallback run, artifact, host, or retry.
"""

import argparse
import hashlib
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
from urllib.parse import urlencode

from extract_artifact import MAX_PACKAGE_BYTES, extract
from select_artifact import (REPOSITORY, SELECTION_FAILURE_REASONS, SelectionFailure,
                             artifact_identity, admit_harness, binary_inputs_changed, read_metadata, select, TARGET)


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
    "artifact_request", "jobs_metadata", "artifact_metadata", "selection",
    "archive_request", "archive_verification", "publication", "unknown",
    "pull_request_request", "pull_request_metadata", "harness_ci_request", "harness_ci_metadata",
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


class ProducerEvidenceUnavailable(SelectionFailure):
    """A complete inventory proves the source evidence is missing or expired."""

    def __init__(self):
        super().__init__("identity", "source subject evidence is unavailable")


def verify_subject(root: Path, ci: dict, source_sha: str, pr_number: int | None,
                   head_sha: str, base_sha: str | None = None) -> dict:
    """Authenticate an actual PR checkout from the existing original subject proof."""
    root.mkdir()
    run_id = ci.get("id")
    if type(run_id) is not int or run_id <= 0:
        raise SelectionFailure("identity", "invalid harness CI identity")
    prefix = f"/repos/{REPOSITORY}/actions"
    gh_api(f"{prefix}/runs/{run_id}/artifacts?per_page=100&page=1",
           root / "harness-artifacts.json", MAX_METADATA_BYTES)
    inventory = read_metadata(root / "harness-artifacts.json")
    rows = inventory.get("artifacts")
    if not isinstance(rows, list) or inventory.get("total_count") != len(rows):
        raise SelectionFailure("identity", "incomplete harness subject inventory")
    candidates = []
    subjects = []
    for item in rows:
        match = re.fullmatch(r"grok-subject-([0-9a-f]{40})-" + str(run_id) + r"-([1-9][0-9]*)",
                             item.get("name", ""))
        if match:
            subjects.append((match, item))
            if type(item.get("expired")) is not bool:
                raise SelectionFailure("identity", "invalid subject availability metadata")
            if match[1] == source_sha and item["expired"] is False:
                candidates.append((int(match[2]), match[1], item))
    if not candidates:
        if subjects and not any(match[1] == source_sha for match, _ in subjects):
            raise SelectionFailure("identity", "subject evidence identifies a different source")
        raise ProducerEvidenceUnavailable()
    candidates.sort(key=lambda item: item[0], reverse=True)
    attempts = [item[0] for item in candidates]
    if len(set(attempts)) != len(attempts):
        raise SelectionFailure("identity", "ambiguous harness subject producer")
    for attempt, subject, artifact in candidates:
        identity = artifact.get("workflow_run", {})
        size, digest, artifact_id = artifact.get("size_in_bytes"), artifact.get("digest", ""), artifact.get("id")
        if (type(artifact_id) is not int or artifact_id <= 0
                or type(size) is not int or not 0 < size <= MAX_METADATA_BYTES
                or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest)
                or identity.get("id") != run_id or identity.get("head_sha") != head_sha):
            raise SelectionFailure("identity", "misidentified harness subject artifact")
        gh_api(f"{prefix}/runs/{run_id}/attempts/{attempt}/jobs?per_page=100&page=1",
               root / f"harness-jobs-{attempt}.json", MAX_METADATA_BYTES)
        jobs = read_metadata(root / f"harness-jobs-{attempt}.json")
        entries = jobs.get("jobs")
        if not isinstance(entries, list) or jobs.get("total_count") != len(entries):
            raise SelectionFailure("identity", "incomplete harness subject job inventory")
        owners = [job for job in entries if job.get("name") == "Native proof / Execution subject"]
        if len(owners) != 1:
            raise SelectionFailure("identity", "ambiguous harness subject job")
        owner = owners[0]
        if (type(owner.get("id")) is not int or owner["id"] <= 0
                or owner.get("run_id") != run_id or owner.get("run_attempt") != attempt):
            raise SelectionFailure("identity", "mismatched harness subject job")
        if owner.get("status") != "completed" or owner.get("conclusion") != "success":
            continue
        archive = root / f"harness-subject-{attempt}.zip"
        gh_api(f"{prefix}/artifacts/{artifact_id}/zip", archive, size)
        if archive.stat().st_size != size or hashlib.sha256(archive.read_bytes()).hexdigest() != digest[7:]:
            raise SelectionFailure("identity", "harness subject archive does not match")
        with zipfile.ZipFile(archive) as zipped:
            entries = zipped.infolist()
            if len(entries) != 1 or entries[0].filename != "grok-subject.json" or not 0 < entries[0].file_size <= MAX_METADATA_BYTES:
                raise SelectionFailure("identity", "invalid harness subject archive")
            report = json.loads(zipped.read(entries[0]))
        if not isinstance(report, dict):
            raise SelectionFailure("identity", "invalid subject report")
        ref = report.get("GITHUB_REF", "")
        match = re.fullmatch(r"refs/pull/([1-9][0-9]*)/merge", ref)
        if match is None or (pr_number is not None and int(match[1]) != pr_number):
            raise SelectionFailure("identity", "subject PR identity does not match")
        actual_pr_number = int(match[1])
        expected = {
            "GITHUB_REPOSITORY": REPOSITORY, "GITHUB_RUN_ID": str(run_id), "GITHUB_RUN_ATTEMPT": str(attempt),
            "GITHUB_EVENT_NAME": "pull_request", "GITHUB_REF": ref,
            "GITHUB_SHA": subject, "GROK_CHECKOUT_SHA": subject, "GROK_SUBJECT_KIND": "pr_test_merge",
            "GITHUB_WORKFLOW_REF": f"{REPOSITORY}/.github/workflows/grok.yml@{ref}",
            "PR_HEAD_SHA": head_sha,
        }
        if (not isinstance(report, dict) or any(report.get(key) != value for key, value in expected.items())
                or not re.fullmatch(r"[0-9a-f]{40}", report.get("GITHUB_WORKFLOW_SHA", ""))
                or not re.fullmatch(r"[0-9a-f]{40}", report.get("PR_BASE_SHA", ""))
                or (base_sha is not None and report.get("PR_BASE_SHA") != base_sha)):
            raise SelectionFailure("identity", "harness CI proved a different PR subject")
        return {"run_id": run_id, "attempt": attempt, "artifact_id": artifact_id,
                "source_sha": subject, "head_sha": head_sha, "base_sha": report["PR_BASE_SHA"],
                "producer_job_id": owner["id"], "pull_request_number": actual_pr_number}
    raise SelectionFailure("identity", "successful subject evidence is unavailable")


def verify_harness_ci_subject(root: Path, ci: dict, pr: dict, harness_sha: str) -> dict:
    proof = verify_subject(root / "harness-proof", ci, pr["merge_commit_sha"], pr["number"],
                           harness_sha, pr["base"]["sha"])
    return {"harness_ci_" + key: value for key, value in proof.items()
            if key not in ("head_sha", "base_sha", "pull_request_number")}


def resolve_producer(root: Path, run: dict, jobs: dict, artifact: dict, artifact_id: int,
                     harness_sha: str, *, target: str = TARGET, expected_line: str | None = None) -> dict:
    """Authenticate producer metadata and its actual checkout, never download the binary."""
    if (run.get("repository", {}).get("full_name") != REPOSITORY
            or run.get("head_repository", {}).get("full_name") != REPOSITORY
            or run.get("path") != ".github/workflows/grok.yml"
            or run.get("event") not in ("push", "pull_request")):
        raise SelectionFailure("identity", "untrusted producer workflow")
    proof = None
    if run["event"] == "pull_request":
        _, _, source = artifact_identity(artifact, artifact_id, target)
        proof = verify_subject(root / "producer-proof", run, source, None, run["head_sha"])
        # After merge run.pull_requests may be empty; the immutable subject owns this ID.
        number = proof["pull_request_number"]
        path = root / "producer-pr.json"
        gh_api(f"/repos/{REPOSITORY}/pulls/{number}", path, MAX_METADATA_BYTES)
        pr = read_metadata(path)
        if (pr.get("number") != number or pr.get("head", {}).get("repo", {}).get("full_name") != REPOSITORY
                or pr.get("base", {}).get("repo", {}).get("full_name") != REPOSITORY):
            raise SelectionFailure("identity", "producer PR is not from the trusted repository")
        proof = {**proof, "line": pr.get("base", {}).get("ref", "")}
    selected = select(run, jobs, artifact, artifact_id, harness_sha, target=target,
                      expected_line=expected_line, producer_source=proof)
    if selected["artifact_size"] > MAX_PACKAGE_BYTES:
        raise ValueError("selected artifact exceeds the package byte budget")
    return selected


def fetch(artifact_id: int, harness_sha: str, output: Path, *,
          expected_line: str | None = None,
          harness_ref: str | None = None) -> dict:
    with acquisition_stage("input"):
        if type(artifact_id) is not int or artifact_id <= 0 or not re.fullmatch(r"[0-9a-f]{40}", harness_sha):
            raise ValueError("invalid explicit artifact or harness revision")
        if ((harness_ref is not None and (not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._/-]*", harness_ref)
                                         or expected_line is not None))
                or (expected_line is not None and not re.fullmatch(r"grok/rust-v[^\s]+", expected_line))):
            raise ValueError("invalid line-bound or PR subject")
        if output.exists() or output.is_symlink():
            raise FileExistsError("output must be a new directory")
    with acquisition_stage("workspace"), tempfile.TemporaryDirectory(
        prefix=".grok-artifact-", dir=output.parent,
    ) as temporary:
        root = Path(temporary)
        prefix = f"/repos/{REPOSITORY}/actions"
        with acquisition_stage("artifact_request"):
            gh_api(f"{prefix}/artifacts/{artifact_id}", root / "artifact.json", MAX_METADATA_BYTES)
        with acquisition_stage("artifact_metadata"):
            artifact = read_metadata(root / "artifact.json")
        with acquisition_stage("selection"):
            run_id, attempt, _ = artifact_identity(artifact, artifact_id)
        with acquisition_stage("run_request"):
            gh_api(f"{prefix}/runs/{run_id}", root / "run.json", MAX_METADATA_BYTES)
        with acquisition_stage("run_metadata"):
            run = read_metadata(root / "run.json")
        pull_request = None
        subject_sha = harness_sha
        harness_evidence = {}
        if harness_ref is not None:
            with acquisition_stage("pull_request_request"):
                query = urlencode({"state": "open", "head": REPOSITORY.split("/")[0] + ":" + harness_ref,
                                   "per_page": 100, "page": 1})
                gh_api(f"/repos/{REPOSITORY}/pulls?{query}", root / "pulls.json", MAX_METADATA_BYTES)
            with acquisition_stage("pull_request_metadata"):
                pulls = json.loads((root / "pulls.json").read_text())
                if not isinstance(pulls, list) or len(pulls) != 1:
                    raise SelectionFailure("identity", "one current PR harness is required")
                number = pulls[0].get("number")
                if type(number) is not int or number <= 0:
                    raise SelectionFailure("identity", "invalid PR harness identity")
            with acquisition_stage("pull_request_request"):
                gh_api(f"/repos/{REPOSITORY}/pulls/{number}", root / "pull-request.json", MAX_METADATA_BYTES)
            with acquisition_stage("pull_request_metadata"):
                pull_request = read_metadata(root / "pull-request.json")
                if pull_request.get("number") != number:
                    raise SelectionFailure("identity", "PR lookup identity changed")
            with acquisition_stage("harness_ci_request"):
                query = urlencode({"event": "pull_request", "head_sha": harness_sha, "per_page": 1, "page": 1})
                gh_api(f"{prefix}/workflows/grok.yml/runs?{query}", root / "harness-ci.json", MAX_METADATA_BYTES)
            with acquisition_stage("harness_ci_metadata"):
                inventory = read_metadata(root / "harness-ci.json")
                runs = inventory.get("workflow_runs")
                if (type(inventory.get("total_count")) is not int or inventory["total_count"] < 1
                        or not isinstance(runs, list) or len(runs) != 1):
                    raise SelectionFailure("identity", "latest PR harness CI is unavailable")
                subject_sha = admit_harness(pull_request, runs[0], harness_sha, harness_ref)
            with acquisition_stage("harness_ci_metadata"):
                harness_evidence = verify_harness_ci_subject(root, runs[0], pull_request, harness_sha)
            with acquisition_stage("selection"):
                subprocess.run(["git", "fetch", "--no-tags", "origin", f"refs/pull/{number}/merge"],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30, check=True)
                merge = subprocess.run(["git", "rev-parse", "FETCH_HEAD"], capture_output=True,
                                       timeout=30, check=True).stdout.decode("ascii").strip()
                if merge != subject_sha:
                    raise SelectionFailure("identity", "PR merge changed during harness admission")
        with acquisition_stage("jobs_request"):
            gh_api(f"{prefix}/runs/{run_id}/attempts/{attempt}/jobs?per_page=100&page=1",
                   root / "jobs.json", MAX_METADATA_BYTES)
        with acquisition_stage("jobs_metadata"):
            jobs = read_metadata(root / "jobs.json")
        with acquisition_stage("selection"):
            selected = resolve_producer(root, run, jobs, artifact, artifact_id, harness_sha,
                                        expected_line=pull_request["base"]["ref"] if pull_request else expected_line)
            if not re.fullmatch(r"grok/rust-v[^\s]+", selected["producer_line"]):
                raise SelectionFailure("identity", "package is outside a supported version line")
            if harness_evidence:
                selected.update(harness_evidence, harness_merge_sha=subject_sha,
                                selection_mode="pr_diagnostic", pull_request_number=pull_request["number"],
                                pr_head_sha=harness_sha)
            if expected_line is not None or pull_request is not None:
                if binary_inputs_changed(Path.cwd(), selected["source_sha"], subject_sha):
                    raise SelectionFailure("identity", "binary inputs do not match the admitted subject")
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
    parser.add_argument("--artifact-id", type=int, required=True)
    parser.add_argument("--harness-sha", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--expected-line")
    parser.add_argument("--harness-ref")
    args = parser.parse_args()
    try:
        result = fetch(args.artifact_id, args.harness_sha, args.output,
                       expected_line=args.expected_line,
                       harness_ref=args.harness_ref)
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

