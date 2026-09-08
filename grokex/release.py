#!/usr/bin/env python3
"""Package Grokex archives from staged binaries and dist files.

Git owns version history. Rust tests and llm-go own correctness.
This helper only lays out install archives.
"""

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

SOURCE_ROOT = Path(__file__).resolve().parent
REPOSITORY_ROOT = SOURCE_ROOT.parent
DIST_ROOT = "grokex/dist"
TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-unknown-linux-musl",
    "aarch64-pc-windows-msvc",
    "x86_64-pc-windows-msvc",
)
LIVE_TARGET = "x86_64-unknown-linux-musl"
LEGACY_KEYS = {
    "model_provider_adapter",
    "model_provider_registrations",
    "provider_adapter",
    "provider_catalog",
}
DIST_FILES = (
    "config.toml.example",
    "INSTALL.md",
    "install-grokex.sh",
    "install-grokex.ps1",
)


def tag_for(version: str) -> str:
    return f"grokex-v{version}"


def archive_name(version: str, target: str) -> str:
    return f"{tag_for(version)}-{target}.tar.gz"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


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
    built_from_sha: str,
    targets: tuple[str, ...] = TARGETS,
) -> None:
    """Stage raw binaries and shipped files into one normalized archive per target."""
    if not version or "/" in version:
        raise SystemExit("version must be a dotted release number")
    if len(built_from_sha) != 40:
        raise SystemExit("built_from_sha must be a 40-character commit SHA")
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
            shutil.copy2(raw / f"codex{suffix}", bin_dir / f"grokex-bin{suffix}")
            shutil.copy2(
                raw / f"codex-code-mode-host{suffix}",
                bin_dir / f"codex-code-mode-host{suffix}",
            )
            if "windows" in target:
                shutil.copy2(dist / "grokex.ps1", bin_dir / "grokex.ps1")
            else:
                shutil.copy2(dist / "grokex", bin_dir / "grokex")
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
                "archive": archive_name(version, target),
                "built_from_sha": built_from_sha,
                "target": target,
                "version": version,
            }
            (stage / "PROVENANCE.json").write_text(
                json.dumps(provenance, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            write_archive(stage, output / archive_name(version, target))


def safe_members(archive: tarfile.TarFile) -> list[tarfile.TarInfo]:
    members = archive.getmembers()
    for member in members:
        path = Path(member.name)
        if path.is_absolute() or ".." in path.parts or member.issym() or member.islnk():
            raise SystemExit(f"unsafe archive member: {member.name}")
    return members


def verify_archives(
    dist: Path,
    version: str,
    built_from_sha: str | None = None,
    targets: tuple[str, ...] = TARGETS,
) -> None:
    """Require exactly the expected archives, each complete for version."""
    expected_archives = {archive_name(version, target) for target in targets}
    actual_archives = {path.name for path in dist.glob("*.tar.gz")}
    if actual_archives != expected_archives:
        raise SystemExit(
            f"archive matrix mismatch: expected {sorted(expected_archives)}, "
            f"got {sorted(actual_archives)}"
        )
    tag = tag_for(version)
    for target in targets:
        path = dist / archive_name(version, target)
        suffix = ".exe" if "windows" in target else ""
        required = {f"{tag}/{filename}" for filename in DIST_FILES} | {
            f"{tag}/LICENSE",
            f"{tag}/PROVENANCE.json",
            f"{tag}/bin/codex-code-mode-host{suffix}",
            f"{tag}/bin/grokex-bin{suffix}",
            f"{tag}/bin/{'grokex.ps1' if suffix else 'grokex'}",
        }
        if "linux" in target:
            required.add(f"{tag}/bin/bwrap")
        with tarfile.open(path, "r:gz") as archive:
            members = safe_members(archive)
            names = {member.name for member in members if member.isfile()}
            missing = required - names
            if missing:
                raise SystemExit(f"{path.name} is missing {sorted(missing)}")
            provenance_member = archive.extractfile(f"{tag}/PROVENANCE.json")
            if provenance_member is None:
                raise SystemExit(f"{path.name} has no provenance")
            provenance = json.loads(provenance_member.read().decode("utf-8"))
        expected = {
            "archive": path.name,
            "target": target,
            "version": version,
        }
        built_from = provenance.get("built_from_sha")
        if not isinstance(built_from, str) or len(built_from) != 40:
            raise SystemExit(f"{path.name} provenance has no build commit")
        if built_from_sha is not None and built_from != built_from_sha:
            raise SystemExit(f"{path.name} provenance mismatch")
        slim = {key: provenance.get(key) for key in ("archive", "target", "version")}
        if slim != expected:
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
    if "model_catalog_json" in config:
        raise SystemExit("profile must use the release-bundled model catalog")
    agents = config.get("agents")
    if isinstance(agents, dict) and agents.get("default_subagent_model") is not None:
        raise SystemExit("profile must not override the default child model")
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


def write_checksums(dist: Path) -> Path:
    archives = sorted(dist.glob("*.tar.gz"))
    if not archives:
        raise SystemExit("no archives to checksum")
    checksums = dist / "SHA256SUMS"
    checksums.write_text(
        "".join(f"{sha256(path)}  {path.name}\n" for path in archives),
        encoding="utf-8",
    )
    return checksums


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    subparsers = parser.add_subparsers(dest="command", required=True)

    package_parser = subparsers.add_parser("package")
    package_parser.add_argument("--raw-root", type=Path, required=True)
    package_parser.add_argument("--output", type=Path, required=True)
    package_parser.add_argument("--repository", type=Path, required=True)
    package_parser.add_argument("--version", required=True)
    package_parser.add_argument("--built-from-sha", required=True)
    package_parser.add_argument("--target", action="append", choices=TARGETS)

    verify_archives_parser = subparsers.add_parser("verify-archives")
    verify_archives_parser.add_argument("--dist", type=Path, required=True)
    verify_archives_parser.add_argument("--version", required=True)
    verify_archives_parser.add_argument("--built-from-sha")
    verify_archives_parser.add_argument("--target", action="append", choices=TARGETS)

    verify_profile_parser = subparsers.add_parser("verify-profile")
    verify_profile_parser.add_argument("--path", type=Path, required=True)
    verify_profile_parser.add_argument("--secret", action="store_true")

    checksums_parser = subparsers.add_parser("checksums")
    checksums_parser.add_argument("--dist", type=Path, required=True)

    args = parser.parse_args()
    if args.command == "package":
        package(
            args.raw_root,
            args.output,
            args.repository,
            args.version,
            args.built_from_sha,
            tuple(args.target) if args.target else TARGETS,
        )
    elif args.command == "verify-archives":
        verify_archives(
            args.dist,
            args.version,
            args.built_from_sha,
            tuple(args.target) if args.target else TARGETS,
        )
    elif args.command == "verify-profile":
        verify_profile(args.path, args.secret)
    elif args.command == "checksums":
        print(write_checksums(args.dist))


if __name__ == "__main__":
    main()
