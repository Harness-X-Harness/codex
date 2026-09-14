#!/usr/bin/env python3
"""Package Grok archives and publish the moving channel."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import shutil
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

DIST_ROOT = "grok/dist"
LIVE_JOB = "Grok Live"
REF_PREFIX = "grok/rust-v"
TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-unknown-linux-musl",
    "aarch64-pc-windows-msvc",
    "x86_64-pc-windows-msvc",
)
DIST_FILES = (
    "config.toml.example",
    "INSTALL.md",
    "install-grok.sh",
    "install-grok.ps1",
)
CHANNEL_FILES = (
    "config.toml.example",
    "install-grok.sh",
    "install-grok.ps1",
)


def version_from_ref(ref: str) -> str:
    value = ref.strip()
    if value.startswith("refs/heads/"):
        value = value.removeprefix("refs/heads/")
    if not value.startswith(REF_PREFIX):
        raise SystemExit(f"ref must start with {REF_PREFIX}")
    version = value.removeprefix(REF_PREFIX)
    if not version or "/" in version:
        raise SystemExit("version must be a dotted release number")
    return version


def tag_for(version: str) -> str:
    return f"grok-v{version}"


def archive_name(version: str, target: str) -> str:
    return f"{tag_for(version)}-{target}.tar.gz"


def artifact_name(sha: str, target: str) -> str:
    return f"grok-build-{sha}-{target}"


def normalized_tar_info(info: tarfile.TarInfo) -> tarfile.TarInfo:
    info.uid = 0
    info.gid = 0
    info.uname = ""
    info.gname = ""
    info.mtime = 0
    return info


def write_archive(source: Path, destination: Path) -> None:
    with destination.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as archive:
                archive.add(
                    source,
                    arcname=source.name,
                    recursive=True,
                    filter=normalized_tar_info,
                )


def package(
    raw_root: Path,
    output: Path,
    repository: Path,
    version: str,
    targets: tuple[str, ...] = TARGETS,
) -> None:
    if not version or "/" in version:
        raise SystemExit("version must be a dotted release number")
    output.mkdir(parents=True, exist_ok=True)
    dist = repository / DIST_ROOT
    tag = tag_for(version)
    for target in targets:
        raw = raw_root / target
        suffix = ".exe" if "windows" in target else ""
        for filename in (f"codex{suffix}", f"codex-code-mode-host{suffix}"):
            if not (raw / filename).is_file():
                raise SystemExit(f"missing raw binary for {target}: {filename}")
        with tempfile.TemporaryDirectory() as temporary:
            stage = Path(temporary) / tag
            bin_dir = stage / "bin"
            bin_dir.mkdir(parents=True)
            for filename in DIST_FILES:
                shutil.copy2(dist / filename, stage / filename)
            shutil.copy2(repository / "LICENSE", stage / "LICENSE")
            shutil.copy2(raw / f"codex{suffix}", bin_dir / f"grok-bin{suffix}")
            shutil.copy2(
                raw / f"codex-code-mode-host{suffix}",
                bin_dir / f"codex-code-mode-host{suffix}",
            )
            if "windows" in target:
                shutil.copy2(dist / "grok.ps1", bin_dir / "grok.ps1")
            else:
                shutil.copy2(dist / "grok", bin_dir / "grok")
                for executable in (
                    bin_dir / "grok",
                    bin_dir / "grok-bin",
                    bin_dir / "codex-code-mode-host",
                    stage / "install-grok.sh",
                ):
                    executable.chmod(0o755)
            if "linux" in target:
                if not (raw / "bwrap").is_file():
                    raise SystemExit(f"missing raw binary for {target}: bwrap")
                shutil.copy2(raw / "bwrap", bin_dir / "bwrap")
                (bin_dir / "bwrap").chmod(0o755)
            write_archive(stage, output / archive_name(version, target))


def _gh(*args: str) -> str:
    return subprocess.check_output(["gh", *args], text=True)


def _gh_json(*args: str):
    return json.loads(_gh(*args))


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _require_proof(repo: str, run_id: str, checkout: Path) -> tuple[str, str, str]:
    run = _gh_json(
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
    version = version_from_ref(branch)
    sha = run["headSha"]
    head = subprocess.check_output(
        ["git", "-C", str(checkout), "rev-parse", "HEAD"], text=True
    ).strip()
    if head != sha:
        raise SystemExit(f"checkout {head} is not proof SHA {sha}")
    ref = _gh_json("api", f"repos/{repo}/git/ref/heads/{branch}")
    if ref["object"]["sha"] != sha:
        raise SystemExit(f"branch {branch} is {ref['object']['sha']}, proof is {sha}")
    jobs = {job["name"]: job for job in run.get("jobs") or []}
    live = jobs.get(LIVE_JOB)
    if live is None or live.get("conclusion") != "success":
        raise SystemExit(f"{LIVE_JOB} did not succeed")
    artifacts = _gh_json("api", f"repos/{repo}/actions/runs/{run_id}/artifacts")
    names = {item["name"] for item in artifacts.get("artifacts") or []}
    missing = [artifact_name(sha, target) for target in TARGETS if artifact_name(sha, target) not in names]
    if missing:
        raise SystemExit(f"missing artifacts: {missing}")
    return branch, version, sha


def _stage_channel(repo_root: Path, run_id: str, repo: str, sha: str, version: str) -> Path:
    staging = Path(tempfile.mkdtemp(prefix="grok-publish-"))
    raw_root = staging / "raw"
    for target in TARGETS:
        dest = raw_root / target
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
                artifact_name(sha, target),
                "--dir",
                str(dest),
            ]
        )
    package(raw_root, staging, repo_root, version)
    dist = repo_root / DIST_ROOT
    for extra in CHANNEL_FILES:
        source = dist / extra
        if not source.is_file():
            raise SystemExit(f"missing {source}")
        shutil.copy2(source, staging / extra)
    lines = [
        f"{_sha256(path)}  {path.name}\n"
        for path in sorted(staging.glob(f"{tag_for(version)}-*.tar.gz"))
    ]
    (staging / "SHA256SUMS").write_text("".join(lines), encoding="utf-8")
    return staging


def _replace_channel(repo: str, version: str, sha: str, staging: Path) -> None:
    tag = tag_for(version)
    viewed = subprocess.call(
        ["gh", "release", "view", tag, "--repo", repo],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if viewed == 0:
        subprocess.check_call(
            ["gh", "release", "delete", tag, "--repo", repo, "--yes", "--cleanup-tag"]
        )
    elif subprocess.call(
        ["gh", "api", f"repos/{repo}/git/ref/tags/{tag}"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    ) == 0:
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


def _readback(repo: str, version: str, sha: str, staging: Path) -> None:
    tag = tag_for(version)
    tag_ref = _gh_json("api", f"repos/{repo}/git/ref/tags/{tag}")
    if tag_ref["object"]["sha"] != sha:
        raise SystemExit(f"tag {tag} is {tag_ref['object']['sha']}, expected {sha}")
    release = _gh_json("api", f"repos/{repo}/releases/tags/{tag}")
    if release.get("target_commitish") != sha:
        raise SystemExit(f"release target is {release.get('target_commitish')}, expected {sha}")
    wanted = sorted(
        [archive_name(version, target) for target in TARGETS] + ["SHA256SUMS", *CHANNEL_FILES]
    )
    actual = sorted(asset["name"] for asset in release["assets"])
    if actual != wanted:
        raise SystemExit(f"assets {actual} != {wanted}")
    remote = {asset["name"]: asset.get("digest") for asset in release["assets"]}
    for path in staging.iterdir():
        if path.is_file() and remote.get(path.name) != f"sha256:{_sha256(path)}":
            raise SystemExit(f"{path.name} digest mismatch")


def publish(repo: str, run_id: str, checkout: Path) -> None:
    branch, version, sha = _require_proof(repo, run_id, checkout)
    staging = _stage_channel(checkout, run_id, repo, sha, version)
    try:
        _replace_channel(repo, version, sha, staging)
        _readback(repo, version, sha, staging)
    except Exception:
        print(
            f"publication of grok-v{version} from {sha} on {branch} failed; no blind retry",
            file=sys.stderr,
        )
        raise
    print(f"published grok-v{version} at {sha}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="cmd", required=True)

    pub = sub.add_parser("publish")
    pub.add_argument("--run-id", required=True)
    pub.add_argument("--repo", required=True)

    args = parser.parse_args()
    publish(args.repo, args.run_id, Path(__file__).resolve().parent.parent)


if __name__ == "__main__":
    main()
