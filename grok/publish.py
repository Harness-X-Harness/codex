#!/usr/bin/env python3
"""Publish a Grok moving channel from a GREEN proof run.

Not invoked by GitHub Actions. Run from a checkout of the proof SHA:

  python3 grok/publish.py --run-id RUN --repo OWNER/NAME
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import release as grok_release

PROOF_JOBS = (
    "Build aarch64-apple-darwin",
    "Build x86_64-apple-darwin",
    "Build aarch64-unknown-linux-musl",
    "Build x86_64-unknown-linux-musl",
    "Build aarch64-pc-windows-msvc",
    "Build x86_64-pc-windows-msvc",
    "Grok Live",
)
CHANNEL_EXTRAS = (
    "config.toml.example",
    "install-grok.sh",
    "install-grok.ps1",
)


def gh(*args: str, text: bool = True) -> str:
    return subprocess.check_output(["gh", *args], text=text)


def gh_json(*args: str):
    return json.loads(gh(*args))


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def require_proof(repo: str, run_id: str) -> tuple[str, str, str]:
    run = gh_json(
        "run",
        "view",
        run_id,
        "--repo",
        repo,
        "--json",
        "conclusion,event,headBranch,headSha,jobs,status,workflowName",
    )
    if run.get("workflowName") != "grok":
        raise SystemExit(f"run {run_id} is not the grok proof workflow")
    if run.get("event") != "push":
        raise SystemExit(f"run {run_id} is not a push proof")
    if run.get("status") != "completed" or run.get("conclusion") != "success":
        raise SystemExit(
            f"run {run_id} is {run.get('status')}/{run.get('conclusion')}"
        )
    branch = run["headBranch"]
    version = grok_release.version_from_ref(branch)
    sha = run["headSha"]
    jobs = {job["name"]: job for job in run.get("jobs") or []}
    missing = [name for name in PROOF_JOBS if name not in jobs]
    if missing:
        raise SystemExit(f"proof jobs missing: {missing}")
    for name in PROOF_JOBS:
        job = jobs[name]
        if job.get("status") != "completed" or job.get("conclusion") != "success":
            raise SystemExit(
                f"{name} is {job.get('status')}/{job.get('conclusion')}"
            )
    ref = gh_json("api", f"repos/{repo}/git/ref/heads/{branch}")
    if ref["object"]["sha"] != sha:
        raise SystemExit(
            f"branch {branch} is {ref['object']['sha']}, proof is {sha}"
        )
    return branch, version, sha


def stage_assets(repo_root: Path, run_id: str, repo: str, sha: str, version: str) -> Path:
    tag = grok_release.tag_for(version)
    staging = Path(tempfile.mkdtemp(prefix="grok-publish-"))
    for target in grok_release.TARGETS:
        name = f"grok-build-{sha}-{target}"
        dest = staging / "download" / target
        dest.mkdir(parents=True)
        subprocess.check_call(
            [
                "gh",
                "run",
                "download",
                run_id,
                "--repo",
                repo,
                "--name",
                name,
                "--dir",
                str(dest),
            ]
        )
        archive = dest / grok_release.archive_name(version, target)
        if not archive.is_file():
            raise SystemExit(f"missing archive {archive.name}")
        archive.replace(staging / archive.name)
    dist = repo_root / grok_release.DIST_ROOT
    for extra in CHANNEL_EXTRAS:
        source = dist / extra
        if not source.is_file():
            raise SystemExit(f"missing {source}")
        (staging / extra).write_bytes(source.read_bytes())
    sums = staging / "SHA256SUMS"
    lines = []
    for archive in sorted(staging.glob(f"{tag}-*.tar.gz")):
        lines.append(f"{sha256_file(archive)}  {archive.name}\n")
    sums.write_text("".join(lines), encoding="utf-8")
    return staging


def expected_names(version: str) -> list[str]:
    tag = grok_release.tag_for(version)
    names = [grok_release.archive_name(version, target) for target in grok_release.TARGETS]
    names.extend(["SHA256SUMS", *CHANNEL_EXTRAS])
    return sorted(names)


def replace_channel(repo: str, version: str, sha: str, staging: Path) -> None:
    tag = grok_release.tag_for(version)
    exists = subprocess.call(
        ["gh", "release", "view", tag, "--repo", repo],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if exists == 0:
        subprocess.check_call(
            [
                "gh",
                "release",
                "delete",
                tag,
                "--repo",
                repo,
                "--yes",
                "--cleanup-tag",
            ]
        )
    else:
        tag_probe = subprocess.call(
            ["gh", "api", f"repos/{repo}/git/ref/tags/{tag}"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        if tag_probe == 0:
            subprocess.check_call(["gh", "api", "-X", "DELETE", f"repos/{repo}/git/ref/tags/{tag}"])
    assets = [str(path) for path in sorted(staging.iterdir()) if path.is_file()]
    subprocess.check_call(
        [
            "gh",
            "release",
            "create",
            tag,
            *assets,
            "--repo",
            repo,
            "--target",
            sha,
            "--title",
            f"Grok {version}",
            "--notes",
            f"Latest GREEN Grok distribution based on stock Codex {version}.",
        ]
    )


def readback(repo: str, version: str, sha: str, staging: Path) -> None:
    tag = grok_release.tag_for(version)
    tag_ref = gh_json("api", f"repos/{repo}/git/ref/tags/{tag}")
    if tag_ref["object"]["sha"] != sha:
        raise SystemExit(f"tag {tag} is {tag_ref['object']['sha']}, expected {sha}")
    release = gh_json("api", f"repos/{repo}/releases/tags/{tag}")
    if release.get("target_commitish") != sha:
        raise SystemExit(
            f"release target is {release.get('target_commitish')}, expected {sha}"
        )
    remote_names = sorted(asset["name"] for asset in release["assets"])
    wanted = expected_names(version)
    if remote_names != wanted:
        raise SystemExit(f"assets {remote_names} != {wanted}")
    remote_digests = {
        asset["name"]: asset.get("digest") for asset in release["assets"]
    }
    for path in staging.iterdir():
        if not path.is_file():
            continue
        expected = f"sha256:{sha256_file(path)}"
        actual = remote_digests.get(path.name)
        if actual != expected:
            raise SystemExit(f"{path.name} digest {actual} != {expected}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--repo", required=True)
    args = parser.parse_args()
    repo_root = Path(__file__).resolve().parent.parent
    head = subprocess.check_output(["git", "-C", str(repo_root), "rev-parse", "HEAD"], text=True).strip()
    branch, version, sha = require_proof(args.repo, args.run_id)
    if head != sha:
        raise SystemExit(f"checkout {head} is not proof SHA {sha}")
    staging = stage_assets(repo_root, args.run_id, args.repo, sha, version)
    try:
        replace_channel(args.repo, version, sha, staging)
        readback(args.repo, version, sha, staging)
    except Exception:
        print(
            f"publication of grok-v{version} from {sha} on {branch} failed; no blind retry",
            file=sys.stderr,
        )
        raise
    print(f"published grok-v{version} at {sha}")


if __name__ == "__main__":
    main()
