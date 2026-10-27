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

from fetch_artifact import MAX_METADATA_BYTES, fetch, gh_api
from select_artifact import REPOSITORY, TARGET, producer_names, read_metadata, select


# Binary inputs are the default. Only clearly consumer-owned areas opt out.
CONSUMER_DIRECTORIES = tuple("grok/" + name + "/" for name in (
    "live", "tests", "facts", "cmd", "internal", "docs",
))
CONSUMER_FILES = frozenset({
    "README.md", "CHANGELOG.md", "grok/go.mod", "grok/go.sum", "grok/TESTING.md",
    "grok/package/fetch_artifact.py", "grok/package/select_artifact.py",
    "grok/package/extract_artifact.py", "grok/package/live_diagnostics.py",
    "grok/package/build_selection.py", "grok/package/verify_provider.sh", "grok/package/README.md", "grok/package/ARTIFACTS.md",
    ".github/workflows/grok.yml", ".github/workflows/grok-live.yml",
    ".github/workflows/grok-facts.yml", ".github/workflows/grok-go.yml",
    ".github/workflows/grok-proof.yml",
})
MAX_INVENTORY = 1000


def is_binary_input(path: str) -> bool:
    if path in CONSUMER_FILES or path.startswith(CONSUMER_DIRECTORIES):
        return False
    if path.startswith("grok/package/test_") and path.endswith(".py") and "/" not in path[len("grok/package/"):]:
        return False
    return True


def ancestor(checkout: Path, source: str, harness: str) -> bool:
    result = subprocess.run(["git", "merge-base", "--is-ancestor", source, harness],
                            cwd=checkout, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                            timeout=30)
    if result.returncode not in (0, 1):
        raise ValueError("source ancestry unavailable")
    return result.returncode == 0


def binary_inputs_changed(checkout: Path, source: str, harness: str) -> bool:
    # Disabling rename detection exposes both the old and new path, including deletes.
    try:
        result = subprocess.run(["git", "diff", "--name-only", "--no-renames", "-z", source, harness, "--"],
                                cwd=checkout, capture_output=True, timeout=30, check=True)
        paths = result.stdout.decode("utf-8", errors="strict").split("\0")
    except (OSError, ValueError, subprocess.SubprocessError):
        return True
    return any(is_binary_input(path) for path in paths if path)


def api(endpoint: str) -> dict:
    with tempfile.TemporaryDirectory(prefix="grok-build-metadata-") as temporary:
        path = Path(temporary) / "metadata.json"
        gh_api(endpoint, path, MAX_METADATA_BYTES)
        return read_metadata(path)


def inventory(endpoint: str, key: str) -> list[dict]:
    """No incomplete inventory may establish the latest available build."""
    rows, total = [], None
    for page in range(1, MAX_INVENTORY // 100 + 1):
        data = api(endpoint + ("&" if "?" in endpoint else "?") + f"per_page=100&page={page}")
        count, batch = data.get("total_count"), data.get(key)
        if (type(count) is not int or not 0 <= count <= MAX_INVENTORY
                or not isinstance(batch, list) or any(not isinstance(row, dict) for row in batch)
                or (total is not None and total != count)):
            raise ValueError("incomplete metadata inventory")
        total = count
        rows.extend(batch)
        if len(rows) == total:
            ids = [row.get("id") for row in rows]
            if any(type(value) is not int or value <= 0 for value in ids) or len(set(ids)) != len(ids):
                raise ValueError("ambiguous metadata inventory")
            return rows
        if not batch or len(rows) > total:
            raise ValueError("incomplete metadata inventory")
    raise ValueError("inventory exceeds bounded selection")


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
    runs = inventory(prefix + "/workflows/grok.yml/runs?" + urlencode({"branch": line, "event": "push"}), "workflow_runs")
    if any(type(run.get("run_number")) is not int or run["run_number"] <= 0 for run in runs):
        raise ValueError("missing producer order")
    if len({run["run_number"] for run in runs}) != len(runs):
        raise ValueError("ambiguous producer order")
    runs.sort(key=lambda item: item["run_number"], reverse=True)
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
        return job

    def choose(target: str) -> dict:
        for run in runs:
            if (run.get("head_branch") != line or run.get("event") != "push"
                    or run.get("path") != ".github/workflows/grok.yml"
                    or run.get("repository", {}).get("full_name") != REPOSITORY
                    or run.get("head_repository", {}).get("full_name") != REPOSITORY
                    or not re.fullmatch(r"[0-9a-f]{40}", run.get("head_sha", ""))):
                raise ValueError("untrusted producer run")
            try:
                eligible = ancestor(checkout, run["head_sha"], harness)
            except (OSError, ValueError, subprocess.SubprocessError):
                return decision(line, harness, reason="source_history_unavailable")
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
                        or type(artifact.get("size_in_bytes")) is not int or artifact["size_in_bytes"] <= 0
                        or not re.fullmatch(r"sha256:[0-9a-f]{64}", artifact.get("digest", ""))):
                    raise ValueError("invalid package metadata")
                # A later failed code build cannot make an old binary prove changed inputs.
                if binary_inputs_changed(checkout, run["head_sha"], harness):
                    return decision(line, harness, reason="binary_inputs_changed")
                if target == TARGET:
                    jobs = rows(prefix + f"/runs/{run['id']}/attempts/{attempt}/jobs", "jobs")
                    select(run, {"total_count": len(jobs), "jobs": jobs}, artifact, artifact["id"], harness,
                           expected_line=line)
                    with tempfile.TemporaryDirectory(prefix="grok-build-package-") as temporary:
                        verified = fetch(artifact["id"], harness, Path(temporary) / "verified", expected_line=line)
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
    except (OSError, ValueError, KeyError, TypeError, AttributeError, subprocess.SubprocessError):
        print("Build subject selection failed; no reuse or build decision was published.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
