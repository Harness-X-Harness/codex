#!/usr/bin/env python3
"""Stage explicit prebuilt inputs using the stock package layout, without builds."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tomllib


REPO_ROOT = Path(__file__).resolve().parents[2]
SUPPORTED_TARGETS = ("x86_64-unknown-linux-musl", "aarch64-apple-darwin")
ASSETS = ("config.toml.example", "models.json", "INSTALL.md", "grok")


def checked_source(source_sha: str) -> str:
    if not re.fullmatch(r"[0-9a-f]{40}", source_sha):
        raise ValueError("source SHA must be a full lowercase commit identity")
    head = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=REPO_ROOT, text=True
    ).strip()
    if head != source_sha:
        raise ValueError("source SHA differs from the checked-out revision")
    required = [
        "grok/package/stage.py", "LICENSE", "codex-rs/Cargo.toml",
        *("grok/dist/" + name for name in ASSETS),
        "scripts/codex_package/layout.py", "scripts/codex_package/targets.py",
        "scripts/codex_package/zsh.py", "scripts/codex_package/dotslash.py",
        "scripts/codex_package/__init__.py",
    ]
    subprocess.run(
        ["git", "ls-files", "--error-unmatch", "--", *required],
        cwd=REPO_ROOT, check=True, stdout=subprocess.DEVNULL,
    )
    owned = ["grok/dist", "grok/package", "scripts/codex_package", "LICENSE", "codex-rs/Cargo.toml"]
    subprocess.run(
        ["git", "diff", "--exit-code", "--quiet", "HEAD", "--", *owned],
        cwd=REPO_ROOT,
        check=True,
    )
    return head


def input_file(path: Path, *, executable: bool) -> Path:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_size == 0:
        raise ValueError("package inputs must be nonempty regular files")
    if executable and not info.st_mode & 0o111:
        raise ValueError("prebuilt executable input lacks executable permission")
    return path


def file_identity(path: Path) -> dict:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return {"size": path.stat().st_size, "sha256": digest.hexdigest()}


def stage(args: argparse.Namespace) -> None:
    source_sha = checked_source(args.source_sha)
    if args.target not in SUPPORTED_TARGETS:
        raise ValueError("unsupported Grok package target")
    if (args.bwrap is not None) != (args.target == "x86_64-unknown-linux-musl"):
        raise ValueError("bwrap is required only for the supported Linux target")
    inputs = [args.runtime, args.code_mode_host, args.rg, args.zsh]
    if args.bwrap is not None:
        inputs.append(args.bwrap)
    for path in inputs:
        input_file(path, executable=True)
    for asset in ASSETS:
        input_file(REPO_ROOT / "grok/dist" / asset, executable=False)
    input_file(REPO_ROOT / "LICENSE", executable=False)
    with (REPO_ROOT / "codex-rs/Cargo.toml").open("rb") as source:
        version = tomllib.load(source)["workspace"]["package"]["version"]

    # Stock owns helper locations and metadata interpretation. These imports do
    # not build or download anything; every executable input above is explicit.
    os.environ["CODEX_REPO_ROOT"] = str(REPO_ROOT)
    sys.path.insert(0, str(REPO_ROOT / "scripts"))
    from codex_package.layout import build_package_dir, validate_package_dir
    from codex_package.targets import PackageInputs, PackageVariant, TARGET_SPECS

    variant = PackageVariant(name="grok", cargo_bin="codex", executable_stem="grok-bin")
    spec = TARGET_SPECS[args.target]
    package_inputs = PackageInputs(
        entrypoint_bin=args.runtime,
        code_mode_host_bin=args.code_mode_host,
        rg_bin=args.rg,
        zsh_bin=args.zsh,
        bwrap_bin=args.bwrap,
        codex_command_runner_bin=None,
        codex_windows_sandbox_setup_bin=None,
    )
    # Exclusive creation preserves any existing directory, file or symlink.
    # A later failure leaves a diagnostic partial directory, never a completed
    # Grok manifest. Retrying requires an explicitly chosen fresh destination.
    args.output.mkdir()
    build_package_dir(args.output, version, variant, spec, package_inputs)
    validate_package_dir(args.output, variant, spec, include_zsh=True)
    for asset in ASSETS:
        destination = args.output / ("bin/grok" if asset == "grok" else asset)
        shutil.copyfile(REPO_ROOT / "grok/dist" / asset, destination)
    (args.output / "bin/grok").chmod(0o755)
    shutil.copyfile(REPO_ROOT / "LICENSE", args.output / "LICENSE")
    files = {
        path.relative_to(args.output).as_posix(): file_identity(path)
        for path in sorted(args.output.rglob("*"))
        if path.is_file()
    }
    metadata = {
        "schema_version": 1,
        "repository": "Harness-X-Harness/codex",
        "source_sha": source_sha,
        "target": args.target,
        "version": version,
        "entrypoint": "bin/grok",
        "runtime": "bin/grok-bin",
        "files": files,
    }
    pending = args.output / ".grok-package.json.tmp"
    with pending.open("x", encoding="utf-8") as output:
        json.dump(metadata, output, indent=2, sort_keys=True)
        output.write("\n")
    pending.rename(args.output / "grok-package.json")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=SUPPORTED_TARGETS, required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--output", type=Path, required=True)
    for name in ("runtime", "code-mode-host", "rg", "zsh"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--bwrap", type=Path)
    args = parser.parse_args()
    try:
        stage(args)
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError):
        print("Package staging failed; no successful package is claimed.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
