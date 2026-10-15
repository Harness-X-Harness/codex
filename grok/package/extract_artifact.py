#!/usr/bin/env python3
"""Verify an identified Actions ZIP and safely extract its one complete package."""

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import sys
import tarfile
import tempfile
import zipfile

from select_artifact import REPOSITORY, TARGET, read_metadata


EXECUTABLES = {
    "bin/grok", "bin/grok-bin", "bin/codex-code-mode-host", "codex-path/rg",
    "codex-resources/zsh/bin/zsh", "codex-resources/bwrap",
}
FILES = EXECUTABLES | {
    "grok-package.json", "codex-package.json", "config.toml.example",
    "models.json", "INSTALL.md", "LICENSE",
}
DIRECTORIES = {".", "bin", "codex-path", "codex-resources", "codex-resources/zsh", "codex-resources/zsh/bin"}
MAX_PACKAGE_BYTES = 2 * 1024 * 1024 * 1024


def digest_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def extract(archive: Path, selected: dict, output: Path) -> dict:
    sha = selected.get("source_sha", "")
    digest = selected.get("artifact_digest", "")
    if (
        selected.get("repository") != REPOSITORY or selected.get("target") != TARGET
        or not re.fullmatch(r"[0-9a-f]{40}", sha)
        or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest)
        or archive.is_symlink() or not archive.is_file()
        or archive.stat().st_size > MAX_PACKAGE_BYTES
        or archive.stat().st_size != selected.get("artifact_size")
        or digest_file(archive) != digest.removeprefix("sha256:")
    ):
        raise ValueError("archive does not match the selected Actions artifact")
    expected_archive = f"grok-{sha}-{TARGET}.tar.gz"
    with zipfile.ZipFile(archive) as zipped, tempfile.TemporaryDirectory() as temporary:
        entries = zipped.infolist()
        if len(entries) != 1 or entries[0].filename != expected_archive or not 0 < entries[0].file_size <= MAX_PACKAGE_BYTES:
            raise ValueError("Actions artifact must contain only the selected package archive")
        package_archive = Path(temporary) / "package.tar.gz"
        with zipped.open(entries[0]) as source, package_archive.open("xb") as target:
            while block := source.read(1024 * 1024):
                target.write(block)
        archive_digest = digest_file(package_archive)
        with tarfile.open(package_archive, "r:gz") as package:
            files = {}
            seen = set()
            total = 0
            for member in package:
                if len(seen) == len(FILES) + len(DIRECTORIES):
                    raise ValueError("unexpected package members")
                relative = PurePosixPath(member.name)
                name = relative.as_posix()
                if relative.is_absolute() or ".." in relative.parts or name in seen:
                    raise ValueError("unsafe or duplicate package member")
                seen.add(name)
                if member.isdir() and name in DIRECTORIES:
                    continue
                if not member.isfile() or name not in FILES or member.size <= 0:
                    raise ValueError("unsupported package member")
                total += member.size
                if total > MAX_PACKAGE_BYTES:
                    raise ValueError("package exceeds extraction budget")
                files[name] = member
            if set(files) != FILES:
                raise ValueError("incomplete package")
            output.mkdir()
            for name, member in files.items():
                destination = output / name
                destination.parent.mkdir(parents=True, exist_ok=True)
                with package.extractfile(member) as source, destination.open("xb") as target:
                    while block := source.read(1024 * 1024):
                        target.write(block)
                destination.chmod(0o600)
    metadata = read_metadata(output / "grok-package.json")
    expected = {
        "schema_version": 1, "repository": REPOSITORY, "source_sha": sha,
        "target": TARGET, "version": "0.158.0", "entrypoint": "bin/grok", "runtime": "bin/grok-bin",
        "files": {
            name: {"size": (output / name).stat().st_size, "sha256": digest_file(output / name)}
            for name in FILES - {"grok-package.json"}
        },
    }
    if metadata != expected:
        raise ValueError("package contents or source/target identity differ from the selected artifact")
    layout = read_metadata(output / "codex-package.json")
    if layout != {"layoutVersion": 1, "version": "0.158.0", "target": TARGET, "variant": "grok",
                  "entrypoint": "bin/grok-bin", "resourcesDir": "codex-resources", "pathDir": "codex-path"}:
        raise ValueError("stock runtime layout differs from the supported package")
    for name in EXECUTABLES:
        (output / name).chmod(0o755)
    return {**selected, "package_archive_sha256": archive_digest,
            "runtime_sha256": metadata["files"]["bin/grok-bin"]["sha256"]}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--selection-json", type=Path, required=True)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = extract(args.archive, read_metadata(args.selection_json), args.output)
    except (OSError, ValueError, KeyError, TypeError, AttributeError, tarfile.TarError, zipfile.BadZipFile):
        print("Package artifact verification failed; no execution is authorized by this result.", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
