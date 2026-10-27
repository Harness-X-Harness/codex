#!/usr/bin/env python3
"""Choose a trusted line package once, or require a new build for its full diff."""

import argparse
import json
from pathlib import Path
import re
import subprocess
import tempfile
import sys
from urllib.parse import urlencode

from fetch_artifact import MAX_METADATA_BYTES, MAX_PACKAGE_BYTES, fetch, gh_api
from select_artifact import (REPOSITORY, TARGET, ancestor, binary_inputs_changed,
                             is_binary_input, producer_names, read_metadata, select)


MAX_INVENTORY = 1000
MAX_RUN_PAGES = 10


class BuildSelectionError(ValueError):
    """A fixed local stage, never an external error message."""

    def __init__(self, stage: str):
        self.stage = stage
        super().__init__(stage)


def api(endpoint: str) -> dict:
    with tempfile.TemporaryDirectory(prefix="grok-build-metadata-") as temporary:
        path = Path(temporary) / "metadata.json"
        try:
            gh_api(endpoint, path, MAX_METADATA_BYTES)
        except (OSError, ValueError, subprocess.SubprocessError):
            raise BuildSelectionError("api_request") from None
        try:
            return read_metadata(path)
        except (OSError, ValueError):
            raise BuildSelectionError("api_metadata") from None


def inventory(endpoint: str, key: str) -> list[dict]:
    """No incomplete inventory may establish the latest available build."""
    rows, total = [], None
    for page in range(1, MAX_INVENTORY // 100 + 1):
        data = api(endpoint + ("&" if "?" in endpoint else "?") + f"per_page=100&page={page}")
        count, batch = data.get("total_count"), data.get(key)
        if (type(count) is not int or not 0 <= count <= MAX_INVENTORY
                or not isinstance(batch, list) or any(not isinstance(row, dict) for row in batch)
                or (total is not None and total != count)):
            raise BuildSelectionError("inventory")
        total = count
        rows.extend(batch)
        if len(rows) == total:
            ids = [row.get("id") for row in rows]
            if any(type(value) is not int or value <= 0 for value in ids) or len(set(ids)) != len(ids):
                raise BuildSelectionError("inventory")
            return rows
        if not batch or len(rows) > total:
            raise BuildSelectionError("inventory")
    raise BuildSelectionError("inventory_budget")


def newest_runs(endpoint: str):
    """Read only the newest pages needed to establish the selected producer.

    Workflow run numbers must arrive strictly newest-first; sorting an incomplete
    response locally could conceal a newer producer. Budget exhaustion is an error,
    never evidence that no package exists.
    """
    seen, previous = set(), None
    for page in range(1, MAX_RUN_PAGES + 1):
        data = api(endpoint + f"&per_page=100&page={page}")
        count, batch = data.get("total_count"), data.get("workflow_runs")
        offset = (page - 1) * 100
        if (type(count) is not int or count < 0 or not isinstance(batch, list)
                or any(not isinstance(row, dict) for row in batch)
                or len(batch) != min(100, max(0, count - offset))):
            raise BuildSelectionError("run_pagination")
        # Validate the complete page before yielding any candidate from it.
        for run in batch:
            identity, number = run.get("id"), run.get("run_number")
            if (type(identity) is not int or identity <= 0 or identity in seen
                    or type(number) is not int or number <= 0
                    or (previous is not None and number >= previous)):
                raise BuildSelectionError("run_order")
            seen.add(identity)
            previous = number
        yield from batch
        if offset + len(batch) >= count:
            return
    raise BuildSelectionError("run_budget")


def decision(line: str, harness: str, *, artifact_id: int | None = None,
             source: str | None = None, reason: str) -> dict:
    return {"line": line, "harness_sha": harness, "build_needed": artifact_id is None,
            "artifact_id": artifact_id, "source_sha": source or harness, "reason": reason}


def plan(line: str, harness: str, checkout: Path, *, include_macos: bool = False) -> dict:
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._/-]*", line) or not re.fullmatch(r"[0-9a-f]{40}", harness):
        raise ValueError("invalid build subject")
    if not re.fullmatch(r"grok/rust-v[0-9][A-Za-z0-9._-]*", line):
        return {**decision(line, harness, reason="no_reusable_version_line"),
                "mac_build_needed": include_macos}
    prefix = f"/repos/{REPOSITORY}/actions"
    run_source = iter(newest_runs(prefix + "/workflows/grok.yml/runs?" + urlencode({"branch": line, "event": "push"})))
    run_cache, run_complete = [], False

    def runs():
        nonlocal run_complete
        index = 0
        while True:
            if index == len(run_cache):
                if run_complete:
                    return
                try:
                    run_cache.append(next(run_source))
                except StopIteration:
                    run_complete = True
                    return
            yield run_cache[index]
            index += 1

    metadata = {}

    def rows(endpoint: str, key: str) -> list[dict]:
        if endpoint not in metadata:
            metadata[endpoint] = inventory(endpoint, key)
        return metadata[endpoint]

    def producer(run: dict, target: str, attempt: int) -> dict | None:
        jobs = rows(prefix + f"/runs/{run['id']}/attempts/{attempt}/jobs", "jobs")
        matches = [job for job in jobs if job.get("name") in producer_names(target)]
        if len(matches) > 1:
            raise ValueError("ambiguous original producer")
        if not matches:
            return None
        job = matches[0]
        if (type(job.get("id")) is not int or job["id"] <= 0
                or job.get("run_id") != run["id"] or job.get("run_attempt") != attempt):
            raise ValueError("mismatched original producer")
        status, conclusion = job.get("status"), job.get("conclusion")
        if (status not in ("queued", "in_progress", "completed", "waiting", "pending", "requested")
                or (status != "completed" and conclusion is not None)
                or (status == "completed" and conclusion not in (
                    "success", "failure", "cancelled", "timed_out", "action_required",
                    "neutral", "skipped", "stale", "startup_failure"))):
            raise ValueError("incomplete producer state")
        return job

    def choose(target: str) -> dict:
        for run in runs():
            if (run.get("head_branch") != line or run.get("event") != "push"
                    or run.get("path") != ".github/workflows/grok.yml"
                    or run.get("repository", {}).get("full_name") != REPOSITORY
                    or run.get("head_repository", {}).get("full_name") != REPOSITORY
                    or not re.fullmatch(r"[0-9a-f]{40}", run.get("head_sha", ""))):
                raise ValueError("untrusted producer run")
            try:
                eligible = ancestor(checkout, run["head_sha"], harness)
            except (OSError, ValueError, subprocess.SubprocessError):
                raise BuildSelectionError("source_history") from None
            if not eligible:
                continue
            artifacts = rows(prefix + f"/runs/{run['id']}/artifacts", "artifacts")
            packages = []
            for artifact in artifacts:
                match = re.fullmatch(r"grok-([0-9a-f]{40})-" + re.escape(target) + r"-([1-9][0-9]*)-([1-9][0-9]*)",
                                     artifact.get("name", ""))
                if match is None:
                    continue
                identity = artifact.get("workflow_run", {})
                if (type(artifact.get("id")) is not int or artifact["id"] <= 0
                        or int(match[2]) != run["id"] or match[1] != run["head_sha"]
                        or identity.get("id") != run["id"] or identity.get("head_sha") != run["head_sha"]):
                    raise ValueError("artifact source mismatch")
                packages.append((int(match[3]), artifact))
            if len({attempt for attempt, _ in packages}) != len(packages):
                raise ValueError("ambiguous producer artifacts")
            for attempt, artifact in sorted(packages, key=lambda pair: pair[0], reverse=True):
                job = producer(run, target, attempt)
                if job is None:
                    raise ValueError("missing original producer")
                if job.get("status") != "completed" or job.get("conclusion") != "success":
                    continue
                if artifact.get("expired") is True:
                    return decision(line, harness, reason="latest_package_unavailable")
                if (artifact.get("expired") is not False
                        or type(artifact.get("size_in_bytes")) is not int
                        or not 0 < artifact["size_in_bytes"] <= MAX_PACKAGE_BYTES
                        or not re.fullmatch(r"sha256:[0-9a-f]{64}", artifact.get("digest", ""))):
                    raise ValueError("invalid package metadata")
                # A later failed code build cannot make an old binary prove changed inputs.
                try:
                    changed = binary_inputs_changed(checkout, run["head_sha"], harness)
                except (OSError, ValueError, subprocess.SubprocessError):
                    raise BuildSelectionError("source_diff") from None
                if changed:
                    return decision(line, harness, reason="binary_inputs_changed")
                if target == TARGET:
                    jobs = rows(prefix + f"/runs/{run['id']}/attempts/{attempt}/jobs", "jobs")
                    select(run, {"total_count": len(jobs), "jobs": jobs}, artifact, artifact["id"], harness,
                           expected_line=line)
                    with tempfile.TemporaryDirectory(prefix="grok-build-package-") as temporary:
                        try:
                            verified = fetch(artifact["id"], harness, Path(temporary) / "verified", expected_line=line)
                        except (OSError, ValueError, subprocess.SubprocessError):
                            raise BuildSelectionError("package_verification") from None
                        if verified["source_sha"] != run["head_sha"] or verified["artifact_id"] != artifact["id"]:
                            raise ValueError("verified package subject changed")
                return decision(line, harness, artifact_id=artifact["id"], source=run["head_sha"], reason="reuse")
            # Without a retained package, a successful newer producer requires rebuilding,
            # not fallback to an older run. A partial rerun with a retained original
            # artifact was resolved by that artifact's own attempt above.
            attempt = run.get("run_attempt")
            if type(attempt) is not int or attempt <= 0:
                raise ValueError("missing current producer observation")
            job = producer(run, target, attempt)
            if job and job.get("status") == "completed" and job.get("conclusion") == "success":
                return decision(line, harness, reason="latest_package_unavailable")
        return decision(line, harness, reason="no_available_package")

    result = choose(TARGET)
    mac = choose("aarch64-apple-darwin") if include_macos else None
    result["mac_build_needed"] = mac["build_needed"] if mac else False
    if mac:
        result["mac_selection"] = mac
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--line", required=True)
    parser.add_argument("--harness-sha", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--github-output", type=Path)
    parser.add_argument("--include-macos", action="store_true")
    args = parser.parse_args()
    try:
        result = plan(args.line, args.harness_sha, Path.cwd(), include_macos=args.include_macos)
        args.output.write_text(json.dumps(result, sort_keys=True) + "\n")
        if args.github_output:
            with args.github_output.open("a") as target:
                target.write("build-needed=" + str(result["build_needed"]).lower() + "\n")
                target.write("mac-build-needed=" + str(result["mac_build_needed"]).lower() + "\n")
                target.write("artifact-id=" + (str(result["artifact_id"]) if result["artifact_id"] else "") + "\n")
                target.write("source-sha=" + result["source_sha"] + "\n")
        print(json.dumps(result, sort_keys=True))
        return 0
    except (OSError, ValueError, KeyError, TypeError, AttributeError, subprocess.SubprocessError) as error:
        reason = error.stage if isinstance(error, BuildSelectionError) else "selection"
        try:
            args.output.write_text(json.dumps({"state": "error", "reason": reason}) + "\n")
        except OSError:
            pass
        print("Build subject selection failed [" + reason + "]; no reuse or build was authorized.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
