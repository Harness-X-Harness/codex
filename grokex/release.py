#!/usr/bin/env python3
"""Grokex release helpers: identity, packaging, manifest, and verification.

The release pipeline is one GitHub Actions run (`grokex-release`): it resolves
the product identity, reuses or builds the six archives, runs every Live
scenario against the Linux archive, and publishes once. The evidence that a
release is valid is that run being green; this module only gives the run its
identities, its archives, its manifest, and an independent check of what was
published.

Identities:

- ``release_tag``/``version``/``upstream_*`` come from ``release-source.json``.
- ``product_tree`` is a digest over the git tree objects of every path that
  feeds a build (``PRODUCT_PATHS``). Two commits with the same product tree
  produce the same product; builds are cached by it, so a commit that changes
  only validators, workflows, or documentation never triggers a rebuild.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
from pathlib import Path

SOURCE_ROOT = Path(__file__).resolve().parent
REPOSITORY_ROOT = SOURCE_ROOT.parent
RELEASE_SOURCE_PATH = SOURCE_ROOT / "release-source.json"
LIVE_CONTRACTS_PATH = SOURCE_ROOT / "live_contracts.json"
DIST_ROOT = "grokex/dist"

# Every path whose content can change a shipped binary or archive. Anything
# outside these paths is validation, tooling, or documentation by definition.
PRODUCT_PATHS = (
    "codex-rs",
    DIST_ROOT,
    "grokex/release-source.json",
    ".github/actions",
    ".github/scripts",
)


def load_release_source(path: Path = RELEASE_SOURCE_PATH) -> dict[str, str]:
    source = json.loads(path.read_text(encoding="utf-8"))
    for key in ("version", "upstream_tag", "upstream_commit"):
        if not isinstance(source.get(key), str) or not source[key]:
            raise SystemExit(f"release-source.json is missing {key}")
    if source["upstream_tag"] != f"rust-v{source['version']}":
        raise SystemExit("release-source.json version does not match the upstream tag")
    return source


def load_live_contracts(path: Path = LIVE_CONTRACTS_PATH) -> dict[str, dict[str, object]]:
    contracts = json.loads(path.read_text(encoding="utf-8"))
    scenarios = contracts.get("scenarios")
    if not isinstance(scenarios, dict) or not scenarios:
        raise SystemExit("live_contracts.json has no scenarios")
    for name, contract in scenarios.items():
        if not isinstance(contract, dict) or not isinstance(contract.get("story"), str):
            raise SystemExit(f"live scenario {name} has no story")
        deadline = contract.get("turn_deadline_seconds")
        if not isinstance(deadline, int) or isinstance(deadline, bool) or deadline <= 0:
            raise SystemExit(f"live scenario {name} has no Turn deadline")
    return scenarios


RELEASE_SOURCE = load_release_source()
VERSION = RELEASE_SOURCE["version"]
TAG = f"grokex-v{VERSION}"
UPSTREAM_TAG = RELEASE_SOURCE["upstream_tag"]
UPSTREAM_COMMIT = RELEASE_SOURCE["upstream_commit"]
SCENARIOS = load_live_contracts()
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
RELEASE_ASSET_DIST_FILES = ("config.toml.example", "install-grokex.sh", "install-grokex.ps1")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git(repository: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=repository, check=True, capture_output=True, text=True
    ).stdout.strip()


def product_tree(repository: Path, revision: str = "HEAD") -> str:
    """Digest of the git tree objects under every product path at revision."""
    digest = hashlib.sha256()
    for path in PRODUCT_PATHS:
        try:
            oid = git(repository, "rev-parse", "--verify", "--quiet", f"{revision}:{path}")
        except subprocess.CalledProcessError as error:
            raise SystemExit(f"product path {path} is missing at {revision}") from error
        digest.update(f"{path}\0{oid}\n".encode())
    return digest.hexdigest()


def identity(repository: Path, revision: str = "HEAD") -> dict[str, str]:
    return {
        "product_tree": product_tree(repository, revision),
        "release_tag": TAG,
        "upstream_commit": UPSTREAM_COMMIT,
        "upstream_tag": UPSTREAM_TAG,
        "version": VERSION,
    }


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


def package(
    raw_root: Path,
    output: Path,
    repository: Path,
    tree: str,
    built_from_sha: str,
    targets: tuple[str, ...] = TARGETS,
) -> None:
    """Stage raw binaries and shipped files into one normalized archive per target.

    ``PROVENANCE.json`` names the product tree the archive implements and the
    commit it was built from; verification compares the tree, so an archive
    built at an earlier commit with the same product tree is the same product.
    """
    output.mkdir(parents=True, exist_ok=True)
    dist = repository / DIST_ROOT
    for target in targets:
        raw = raw_root / target
        suffix = ".exe" if "windows" in target else ""
        for filename in (f"codex{suffix}", f"codex-code-mode-host{suffix}"):
            if not (raw / filename).is_file():
                raise SystemExit(f"missing raw binary for {target}: {filename}")

        with tempfile.TemporaryDirectory() as temporary:
            stage = Path(temporary) / TAG
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
                "archive": archive_name(target),
                "built_from_sha": built_from_sha,
                "product_tree": tree,
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


def verify_archives(dist: Path, tree: str, targets: tuple[str, ...] = TARGETS) -> None:
    """Require exactly the expected archives, each complete and implementing tree."""
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
        required = {f"{TAG}/{filename}" for filename in DIST_FILES} | {
            f"{TAG}/LICENSE",
            f"{TAG}/PROVENANCE.json",
            f"{TAG}/bin/codex-code-mode-host{suffix}",
            f"{TAG}/bin/grokex-bin{suffix}",
            f"{TAG}/bin/{'grokex.ps1' if suffix else 'grokex'}",
        }
        if "linux" in target:
            required.add(f"{TAG}/bin/bwrap")
        with tarfile.open(path, "r:gz") as archive:
            members = safe_members(archive)
            names = {member.name for member in members if member.isfile()}
            missing = required - names
            if missing:
                raise SystemExit(f"{path.name} is missing {sorted(missing)}")
            provenance_file = archive.extractfile(f"{TAG}/PROVENANCE.json")
            if provenance_file is None:
                raise SystemExit(f"{path.name} has no provenance")
            provenance = json.load(provenance_file)
        built_from = provenance.pop("built_from_sha", None)
        if not isinstance(built_from, str) or len(built_from) != 40:
            raise SystemExit(f"{path.name} provenance has no build commit")
        expected = {
            "archive": path.name,
            "product_tree": tree,
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


def scenario_summary(evidence_dir: Path) -> dict[str, dict[str, object]]:
    """Summarize the evidence files the release run's own Live jobs wrote.

    Every contract scenario must be present, in release mode, and completed;
    the Live jobs already gate the run, so this is a consistency check of what
    the manifest is about to claim, not a second oracle.
    """
    summary: dict[str, dict[str, object]] = {}
    for path in sorted(evidence_dir.glob("*.json")):
        value = json.loads(path.read_text(encoding="utf-8"))
        scenario = value.get("scenario")
        if scenario not in SCENARIOS or scenario in summary:
            raise SystemExit(f"unexpected live evidence file: {path.name}")
        if value.get("mode") != "release" or value.get("status") != "completed":
            raise SystemExit(f"live scenario is not a completed release proof: {scenario}")
        if value.get("story") != SCENARIOS[scenario]["story"]:
            raise SystemExit(f"live scenario Story mismatch: {scenario}")
        summary[scenario] = {
            "status": "completed",
            "story": value["story"],
            "turn_durations_seconds": value.get("turn_durations_seconds", []),
        }
    missing = set(SCENARIOS) - set(summary)
    if missing:
        raise SystemExit(f"live evidence is incomplete: {sorted(missing)}")
    return summary


def manifest(source_sha: str, tree: str, release_run: str, scenarios: dict[str, dict[str, object]]) -> dict[str, object]:
    return {
        "archives": [archive_name(target) for target in TARGETS],
        "live_archive": archive_name(LIVE_TARGET),
        "product_tree": tree,
        "release_run": release_run,
        "scenarios": scenarios,
        "source_sha": source_sha,
        "tag": TAG,
        "upstream_commit": UPSTREAM_COMMIT,
        "version": VERSION,
    }


def assemble(
    archives: Path,
    evidence_dir: Path,
    output: Path,
    repository: Path,
    source_sha: str,
    tree: str,
    release_run: str,
) -> None:
    """Compose the release asset set: six archives, dist files, RELEASE.json, SHA256SUMS."""
    output.mkdir(parents=True, exist_ok=True)
    verify_archives(archives, tree)
    for target in TARGETS:
        shutil.copy2(archives / archive_name(target), output / archive_name(target))
    for filename in RELEASE_ASSET_DIST_FILES:
        shutil.copy2(repository / DIST_ROOT / filename, output / filename)
    scenarios = scenario_summary(evidence_dir)
    (output / "RELEASE.json").write_text(
        json.dumps(manifest(source_sha, tree, release_run, scenarios), indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    checksummed = sorted(path for path in output.iterdir() if path.name != "SHA256SUMS")
    (output / "SHA256SUMS").write_text(
        "".join(f"{sha256(path)}  {path.name}\n" for path in checksummed),
        encoding="utf-8",
    )


def verify_assets(dist: Path, source_sha: str, tree: str, release_run: str) -> None:
    """Check a published (or about-to-publish) asset set against the identities."""
    expected = {archive_name(target) for target in TARGETS} | set(RELEASE_ASSET_DIST_FILES) | {
        "RELEASE.json",
        "SHA256SUMS",
    }
    actual = {path.name for path in dist.iterdir() if path.is_file()}
    if actual != expected:
        raise SystemExit(f"release asset mismatch: expected {sorted(expected)}, got {sorted(actual)}")
    verify_archives(dist, tree)
    published = json.loads((dist / "RELEASE.json").read_text(encoding="utf-8"))
    scenarios = published.get("scenarios")
    if not isinstance(scenarios, dict) or set(scenarios) != set(SCENARIOS):
        raise SystemExit("release manifest scenario set mismatch")
    for scenario, value in scenarios.items():
        if not isinstance(value, dict) or value.get("status") != "completed":
            raise SystemExit(f"release manifest scenario is not completed: {scenario}")
        if value.get("story") != SCENARIOS[scenario]["story"]:
            raise SystemExit(f"release manifest Story mismatch: {scenario}")
    if published != manifest(source_sha, tree, release_run, scenarios):
        raise SystemExit("release manifest mismatch")
    recorded: dict[str, str] = {}
    for line in (dist / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        digest, filename = line.split("  ", 1)
        recorded[filename] = digest
    if set(recorded) != expected - {"SHA256SUMS"}:
        raise SystemExit("checksum manifest file set mismatch")
    for filename, digest in recorded.items():
        if sha256(dist / filename) != digest:
            raise SystemExit(f"checksum mismatch: {filename}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    subparsers = parser.add_subparsers(dest="command", required=True)

    identity_parser = subparsers.add_parser("identity", help="print release and product identities")
    identity_parser.add_argument("--repository", type=Path, default=REPOSITORY_ROOT)
    identity_parser.add_argument("--revision", default="HEAD")
    identity_parser.add_argument("--github-output", action="store_true")

    package_parser = subparsers.add_parser("package")
    package_parser.add_argument("--raw-root", type=Path, required=True)
    package_parser.add_argument("--output", type=Path, required=True)
    package_parser.add_argument("--repository", type=Path, required=True)
    package_parser.add_argument("--product-tree", required=True)
    package_parser.add_argument("--built-from-sha", required=True)
    package_parser.add_argument("--target", action="append", choices=TARGETS)

    verify_archives_parser = subparsers.add_parser("verify-archives")
    verify_archives_parser.add_argument("--dist", type=Path, required=True)
    verify_archives_parser.add_argument("--product-tree", required=True)
    verify_archives_parser.add_argument("--target", action="append", choices=TARGETS)

    verify_profile_parser = subparsers.add_parser("verify-profile")
    verify_profile_parser.add_argument("--path", type=Path, required=True)
    verify_profile_parser.add_argument("--secret", action="store_true")

    assemble_parser = subparsers.add_parser("assemble")
    assemble_parser.add_argument("--archives", type=Path, required=True)
    assemble_parser.add_argument("--evidence-dir", type=Path, required=True)
    assemble_parser.add_argument("--output", type=Path, required=True)
    assemble_parser.add_argument("--repository", type=Path, required=True)
    assemble_parser.add_argument("--source-sha", required=True)
    assemble_parser.add_argument("--product-tree", required=True)
    assemble_parser.add_argument("--release-run", required=True)

    verify_assets_parser = subparsers.add_parser("verify-assets")
    verify_assets_parser.add_argument("--dist", type=Path, required=True)
    verify_assets_parser.add_argument("--source-sha", required=True)
    verify_assets_parser.add_argument("--product-tree", required=True)
    verify_assets_parser.add_argument("--release-run", required=True)

    args = parser.parse_args()
    if args.command == "identity":
        values = identity(args.repository, args.revision)
        if args.github_output:
            for key, value in sorted(values.items()):
                print(f"{key}={value}")
        else:
            print(json.dumps(values, indent=2, sort_keys=True))
    elif args.command == "package":
        package(
            args.raw_root,
            args.output,
            args.repository,
            args.product_tree,
            args.built_from_sha,
            tuple(args.target) if args.target else TARGETS,
        )
    elif args.command == "verify-archives":
        verify_archives(args.dist, args.product_tree, tuple(args.target) if args.target else TARGETS)
    elif args.command == "verify-profile":
        verify_profile(args.path, args.secret)
    elif args.command == "assemble":
        assemble(
            args.archives,
            args.evidence_dir,
            args.output,
            args.repository,
            args.source_sha,
            args.product_tree,
            args.release_run,
        )
    elif args.command == "verify-assets":
        verify_assets(args.dist, args.source_sha, args.product_tree, args.release_run)


if __name__ == "__main__":
    main()
