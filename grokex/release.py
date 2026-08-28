#!/usr/bin/env python3
"""Build and verify the small Grokex release envelope around Codex binaries."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import shutil
import tarfile
import tempfile
import tomllib
from pathlib import Path


VERSION = "0.149.0"
TAG = f"grokex-v{VERSION}"
UPSTREAM_COMMIT = "758ef40f50c1a458425c7cfbf1eb12cbc07af0b0"
TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-unknown-linux-musl",
    "aarch64-pc-windows-msvc",
    "x86_64-pc-windows-msvc",
)
LEGACY_KEYS = {
    "model_provider_adapter",
    "model_provider_registrations",
    "provider_adapter",
    "provider_catalog",
}
LIVE_SCENARIO_ASSERTIONS = {
    "basic-exact-reply": {
        "response_assertion": "exact_match",
        "runner_turn_submission_count": 1,
        "semantic_acceptance": "proven",
        "status": "completed",
    },
    "encrypted-reasoning-tool-continuation": {
        "history_response_assertion": "exact_match",
        "reasoning_replay": "completed",
        "response_assertion": "exact_match",
        "runner_turn_submission_count": 2,
        "semantic_acceptance": "proven",
        "status": "completed",
        "tool_continuation": "completed",
    },
    "ultra-full-history-collaboration": {
        "child_completion": "completed",
        "child_response_assertion": "exact_match",
        "default_full_history": "completed",
        "parent_completion": "completed",
        "parent_result_consumption": "completed",
        "response_assertion": "exact_match",
        "runner_turn_submission_count": 1,
        "semantic_acceptance": "proven",
        "status": "completed",
        "wait_path": "completed",
    },
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def archive_name(target: str) -> str:
    return f"{TAG}-{target}.tar.gz"


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


def copy_release_files(stage: Path, repository: Path) -> None:
    for relative in (
        "grokex/config.toml.example",
        "grokex/INSTALL.md",
        "grokex/install-grokex.sh",
        "grokex/install-grokex.ps1",
        "LICENSE",
    ):
        shutil.copy2(repository / relative, stage / Path(relative).name)


def package(
    raw_root: Path,
    output: Path,
    repository: Path,
    source_sha: str,
    targets: tuple[str, ...] = TARGETS,
) -> None:
    output.mkdir(parents=True, exist_ok=True)
    for target in targets:
        raw = raw_root / target
        suffix = ".exe" if "windows" in target else ""
        required = (f"codex{suffix}", f"codex-code-mode-host{suffix}")
        for filename in required:
            if not (raw / filename).is_file():
                raise SystemExit(f"missing raw binary for {target}: {filename}")

        with tempfile.TemporaryDirectory() as temporary:
            stage = Path(temporary) / TAG
            bin_dir = stage / "bin"
            bin_dir.mkdir(parents=True)
            copy_release_files(stage, repository)

            shutil.copy2(raw / f"codex{suffix}", bin_dir / f"grokex-bin{suffix}")
            shutil.copy2(
                raw / f"codex-code-mode-host{suffix}",
                bin_dir / f"codex-code-mode-host{suffix}",
            )
            if "windows" in target:
                shutil.copy2(repository / "grokex/grokex.ps1", bin_dir / "grokex.ps1")
            else:
                shutil.copy2(repository / "grokex/grokex", bin_dir / "grokex")
                for executable in (
                    bin_dir / "grokex",
                    bin_dir / "grokex-bin",
                    bin_dir / "codex-code-mode-host",
                    stage / "install-grokex.sh",
                ):
                    executable.chmod(0o755)

            if "linux" in target:
                if not (raw / "bwrap").is_file():
                    raise SystemExit(f"missing raw binary for {target}: bwrap")
                shutil.copy2(raw / "bwrap", bin_dir / "bwrap")
                (bin_dir / "bwrap").chmod(0o755)

            provenance = {
                "archive": archive_name(target),
                "source_sha": source_sha,
                "target": target,
                "upstream_commit": UPSTREAM_COMMIT,
                "version": VERSION,
            }
            (stage / "PROVENANCE.json").write_text(
                json.dumps(provenance, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            write_archive(stage, output / archive_name(target))


def safe_members(archive: tarfile.TarFile) -> list[tarfile.TarInfo]:
    members = archive.getmembers()
    for member in members:
        path = Path(member.name)
        if path.is_absolute() or ".." in path.parts or member.issym() or member.islnk():
            raise SystemExit(f"unsafe archive member: {member.name}")
    return members


def verify_archives(
    dist: Path,
    source_sha: str,
    targets: tuple[str, ...] = TARGETS,
) -> None:
    expected_archives = {archive_name(target) for target in targets}
    actual_archives = {path.name for path in dist.glob("*.tar.gz")}
    if actual_archives != expected_archives:
        raise SystemExit(
            f"archive matrix mismatch: expected {sorted(expected_archives)}, "
            f"got {sorted(actual_archives)}"
        )

    for target in targets:
        path = dist / archive_name(target)
        suffix = ".exe" if "windows" in target else ""
        root = TAG
        required = {
            f"{root}/INSTALL.md",
            f"{root}/LICENSE",
            f"{root}/PROVENANCE.json",
            f"{root}/config.toml.example",
            f"{root}/install-grokex.ps1",
            f"{root}/install-grokex.sh",
            f"{root}/bin/codex-code-mode-host{suffix}",
            f"{root}/bin/grokex-bin{suffix}",
            f"{root}/bin/{'grokex.ps1' if suffix else 'grokex'}",
        }
        if "linux" in target:
            required.add(f"{root}/bin/bwrap")

        with tarfile.open(path, "r:gz") as archive:
            members = safe_members(archive)
            names = {member.name for member in members if member.isfile()}
            missing = required - names
            if missing:
                raise SystemExit(f"{path.name} is missing {sorted(missing)}")
            provenance_file = archive.extractfile(f"{root}/PROVENANCE.json")
            if provenance_file is None:
                raise SystemExit(f"{path.name} has no provenance")
            provenance = json.load(provenance_file)
        expected = {
            "archive": path.name,
            "source_sha": source_sha,
            "target": target,
            "upstream_commit": UPSTREAM_COMMIT,
            "version": VERSION,
        }
        if provenance != expected:
            raise SystemExit(f"{path.name} provenance mismatch")


def find_legacy_key(value: object) -> str | None:
    if isinstance(value, dict):
        for key, child in value.items():
            if key in LEGACY_KEYS:
                return key
            found = find_legacy_key(child)
            if found:
                return found
    elif isinstance(value, list):
        for child in value:
            found = find_legacy_key(child)
            if found:
                return found
    return None


def verify_profile(path: Path, secret: bool) -> None:
    with path.open("rb") as handle:
        config = tomllib.load(handle)
    legacy = find_legacy_key(config)
    if legacy:
        raise SystemExit(f"profile contains unsupported authority: {legacy}")
    if config.get("model") != "grok-4.6" or config.get("model_provider") != "grok":
        raise SystemExit("profile must select exact grok-4.6 from Provider grok")
    providers = config.get("model_providers")
    provider = providers.get("grok") if isinstance(providers, dict) else None
    if not isinstance(provider, dict):
        raise SystemExit("profile has no model_providers.grok table")
    required = {
        "base_url": "https://grok.trustedtunnel.app/v1",
        "wire_api": "grok_responses",
        "requires_openai_auth": False,
        "supports_websockets": False,
    }
    if any(provider.get(key) != value for key, value in required.items()):
        raise SystemExit("profile does not match the supported Grok transport contract")
    env_auth = provider.get("env_key") == "GROK_API_KEY"
    token_auth = secret and bool(provider.get("experimental_bearer_token"))
    if not (env_auth or token_auth):
        raise SystemExit("profile has no supported Grok bearer-token authority")
    if not secret and provider.get("experimental_bearer_token") is not None:
        raise SystemExit("public profile must not contain a bearer token")


def build_live_evidence(
    evidence_dir: Path,
    archive: Path,
    output: Path,
    source_sha: str,
    validator_sha: str,
    run_id: str,
) -> None:
    archive_digest = sha256(archive)
    common = {
        "archive": archive.name,
        "archive_sha256": archive_digest,
        "catalog": "release-bundled",
        "model": "grok-4.6",
        "multi_agent_version": "v2",
        "provider": "grok",
        "reasoning_effort": "ultra",
        "source_sha": source_sha,
        "validation_run": run_id,
        "validator_sha": validator_sha,
    }
    observed: dict[str, dict[str, object]] = {}
    for path in evidence_dir.glob("*.json"):
        value = json.loads(path.read_text(encoding="utf-8"))
        scenario = value.get("scenario")
        if scenario not in LIVE_SCENARIO_ASSERTIONS or scenario in observed:
            raise SystemExit("live scenario evidence set is invalid")
        if any(value.get(key) != expected for key, expected in common.items()):
            raise SystemExit(f"live scenario evidence mismatch: {scenario}")
        expected_assertions = LIVE_SCENARIO_ASSERTIONS[scenario]
        if any(value.get(key) != expected for key, expected in expected_assertions.items()):
            raise SystemExit(f"live scenario outcome mismatch: {scenario}")
        observed[scenario] = expected_assertions
    if set(observed) != set(LIVE_SCENARIO_ASSERTIONS):
        raise SystemExit("required live scenario evidence is incomplete")

    manifest = {
        "archive": archive.name,
        "archive_sha256": archive_digest,
        "catalog": "release-bundled",
        "model": "grok-4.6",
        "multi_agent_version": "v2",
        "runner_turn_submission_count": sum(
            assertions["runner_turn_submission_count"]
            for assertions in observed.values()
        ),
        "provider": "grok",
        "reasoning_effort": "ultra",
        "scenarios": observed,
        "source_sha": source_sha,
        "status": "completed",
        "validation_run": run_id,
        "validator_sha": validator_sha,
    }
    output.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def build_assets(
    archives: Path,
    evidence: Path,
    output: Path,
    repository: Path,
    source_sha: str,
    run_id: str,
) -> None:
    output.mkdir(parents=True, exist_ok=True)
    verify_archives(archives, source_sha)
    for target in TARGETS:
        shutil.copy2(archives / archive_name(target), output / archive_name(target))
    for relative in (
        "grokex/config.toml.example",
        "grokex/install-grokex.sh",
        "grokex/install-grokex.ps1",
    ):
        shutil.copy2(repository / relative, output / Path(relative).name)
    shutil.copy2(evidence, output / "LIVE_EVIDENCE.json")

    manifest = {
        "archives": [archive_name(target) for target in TARGETS],
        "source_sha": source_sha,
        "tag": TAG,
        "upstream_commit": UPSTREAM_COMMIT,
        "validation_run": run_id,
        "version": VERSION,
    }
    (output / "RELEASE.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    checksummed = sorted(path for path in output.iterdir() if path.name != "SHA256SUMS")
    (output / "SHA256SUMS").write_text(
        "".join(f"{sha256(path)}  {path.name}\n" for path in checksummed),
        encoding="utf-8",
    )


def verify_assets(dist: Path, source_sha: str, run_id: str) -> None:
    expected = {archive_name(target) for target in TARGETS} | {
        "LIVE_EVIDENCE.json",
        "RELEASE.json",
        "SHA256SUMS",
        "config.toml.example",
        "install-grokex.ps1",
        "install-grokex.sh",
    }
    actual = {path.name for path in dist.iterdir() if path.is_file()}
    if actual != expected:
        raise SystemExit(f"release asset mismatch: expected {sorted(expected)}, got {sorted(actual)}")
    verify_archives(dist, source_sha)

    manifest = json.loads((dist / "RELEASE.json").read_text(encoding="utf-8"))
    expected_manifest = {
        "archives": [archive_name(target) for target in TARGETS],
        "source_sha": source_sha,
        "tag": TAG,
        "upstream_commit": UPSTREAM_COMMIT,
        "validation_run": run_id,
        "version": VERSION,
    }
    if manifest != expected_manifest:
        raise SystemExit("release manifest mismatch")

    evidence = json.loads((dist / "LIVE_EVIDENCE.json").read_text(encoding="utf-8"))
    live_archive = archive_name("x86_64-unknown-linux-musl")
    required_evidence = {
        "archive": live_archive,
        "runner_turn_submission_count": sum(
            assertions["runner_turn_submission_count"]
            for assertions in LIVE_SCENARIO_ASSERTIONS.values()
        ),
        "provider": "grok",
        "scenarios": LIVE_SCENARIO_ASSERTIONS,
        "source_sha": source_sha,
        "status": "completed",
        "validation_run": run_id,
    }
    if any(evidence.get(key) != value for key, value in required_evidence.items()):
        raise SystemExit("live evidence mismatch")
    if evidence.get("archive_sha256") != sha256(dist / live_archive):
        raise SystemExit("live evidence archive checksum mismatch")

    checksum_lines = (dist / "SHA256SUMS").read_text(encoding="utf-8").splitlines()
    recorded: dict[str, str] = {}
    for line in checksum_lines:
        digest, filename = line.split("  ", 1)
        recorded[filename] = digest
    if set(recorded) != expected - {"SHA256SUMS"}:
        raise SystemExit("checksum manifest file set mismatch")
    for filename, digest in recorded.items():
        if sha256(dist / filename) != digest:
            raise SystemExit(f"checksum mismatch: {filename}")


def main() -> None:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    package_parser = subparsers.add_parser("package")
    package_parser.add_argument("--raw-root", type=Path, required=True)
    package_parser.add_argument("--output", type=Path, required=True)
    package_parser.add_argument("--repository", type=Path, required=True)
    package_parser.add_argument("--source-sha", required=True)
    package_parser.add_argument("--target", action="append", choices=TARGETS)

    verify_archives_parser = subparsers.add_parser("verify-archives")
    verify_archives_parser.add_argument("--dist", type=Path, required=True)
    verify_archives_parser.add_argument("--source-sha", required=True)
    verify_archives_parser.add_argument("--target", action="append", choices=TARGETS)

    profile_parser = subparsers.add_parser("verify-profile")
    profile_parser.add_argument("--path", type=Path, required=True)
    profile_parser.add_argument("--secret", action="store_true")

    evidence_parser = subparsers.add_parser("build-live-evidence")
    evidence_parser.add_argument("--evidence-dir", type=Path, required=True)
    evidence_parser.add_argument("--archive", type=Path, required=True)
    evidence_parser.add_argument("--output", type=Path, required=True)
    evidence_parser.add_argument("--source-sha", required=True)
    evidence_parser.add_argument("--validator-sha", required=True)
    evidence_parser.add_argument("--run-id", required=True)

    assets_parser = subparsers.add_parser("build-assets")
    assets_parser.add_argument("--archives", type=Path, required=True)
    assets_parser.add_argument("--evidence", type=Path, required=True)
    assets_parser.add_argument("--output", type=Path, required=True)
    assets_parser.add_argument("--repository", type=Path, required=True)
    assets_parser.add_argument("--source-sha", required=True)
    assets_parser.add_argument("--run-id", required=True)

    verify_assets_parser = subparsers.add_parser("verify-assets")
    verify_assets_parser.add_argument("--dist", type=Path, required=True)
    verify_assets_parser.add_argument("--source-sha", required=True)
    verify_assets_parser.add_argument("--run-id", required=True)

    args = parser.parse_args()
    if args.command == "package":
        package(
            args.raw_root,
            args.output,
            args.repository,
            args.source_sha,
            tuple(args.target) if args.target else TARGETS,
        )
    elif args.command == "verify-archives":
        verify_archives(
            args.dist,
            args.source_sha,
            tuple(args.target) if args.target else TARGETS,
        )
    elif args.command == "verify-profile":
        verify_profile(args.path, args.secret)
    elif args.command == "build-live-evidence":
        build_live_evidence(
            args.evidence_dir,
            args.archive,
            args.output,
            args.source_sha,
            args.validator_sha,
            args.run_id,
        )
    elif args.command == "build-assets":
        build_assets(
            args.archives,
            args.evidence,
            args.output,
            args.repository,
            args.source_sha,
            args.run_id,
        )
    elif args.command == "verify-assets":
        verify_assets(args.dist, args.source_sha, args.run_id)


if __name__ == "__main__":
    main()
