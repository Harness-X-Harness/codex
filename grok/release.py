#!/usr/bin/env python3
"""Package Grok archives from staged binaries and dist files."""

from __future__ import annotations

import argparse
import gzip
import shutil
import tarfile
import tempfile
from pathlib import Path

SOURCE_ROOT = Path(__file__).resolve().parent
DIST_ROOT = "grok/dist"
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


def tag_for(version: str) -> str:
    return f"grok-v{version}"


def archive_name(version: str, target: str) -> str:
    return f"{tag_for(version)}-{target}.tar.gz"


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
    """Stage raw binaries and shipped files into one normalized archive per target."""
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


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--raw-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repository", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", action="append", choices=TARGETS)
    args = parser.parse_args()

    package(
        args.raw_root,
        args.output,
        args.repository,
        args.version,
        tuple(args.target) if args.target else TARGETS,
    )


if __name__ == "__main__":
    main()
