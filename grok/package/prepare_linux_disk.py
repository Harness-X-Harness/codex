#!/usr/bin/env python3
"""Reclaim unused image SDKs only on the hosted Linux package builder."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


# Fixed runner-image installations, not caller-supplied paths or build inputs.
# This Rust/Go package job does not use Android, .NET, or Haskell.
IMAGE_SDKS = (
    Path("/usr/local/lib/android"),
    Path("/usr/share/dotnet"),
    Path("/usr/local/.ghcup"),
)


def report(message, *, error=False):
    try:
        # Avoid a buffered stream's failing shutdown flush changing our exit.
        os.write(2 if error else 1, ("grok-build-disk " + message + "\n").encode())
    except OSError:
        pass


def report_disk(phase):
    # Measurement is diagnostic, not an additional build acceptance threshold.
    available = None
    try:
        available = shutil.disk_usage(os.environ["CARGO_TARGET_DIR"]).free
    except (KeyError, OSError):
        pass
    report(json.dumps({"phase": phase, "available_bytes": available}, sort_keys=True))


def main():
    expected = {
        "GITHUB_ACTIONS": "true",
        "RUNNER_ENVIRONMENT": "github-hosted",
        "RUNNER_OS": "Linux",
        "TARGET": "x86_64-unknown-linux-musl",
    }
    if any(os.environ.get(key) != value for key, value in expected.items()):
        report("cleanup_skipped_outside_hosted_linux_package")
        return 0

    # Validate every present root before deleting any of them. In particular,
    # never follow an image path or ancestor redirected to another directory.
    present = []
    for path in IMAGE_SDKS:
        if not path.exists() and not path.is_symlink():
            continue
        if path.resolve() != path or not path.is_dir():
            report("unsafe_image_sdk_path", error=True)
            return 1
        present.append(path)

    report_disk("before")
    try:
        for path in present:
            subprocess.run(
                ["sudo", "-n", "rm", "-rf", "--one-file-system", "--preserve-root=all", "--", str(path)],
                check=True,
            )
    except (OSError, subprocess.CalledProcessError):
        report("image_sdk_cleanup_failed", error=True)
        return 1
    finally:
        report_disk("after")
    return 0


if __name__ == "__main__":
    sys.exit(main())
