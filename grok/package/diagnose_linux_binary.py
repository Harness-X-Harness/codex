#!/usr/bin/env python3
"""Record bounded, credential-free release startup diagnostics; never accept a failed probe."""

import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import resource
import shutil
import signal
import subprocess


PHASES = ("before-strip", "after-strip", "staged")
MAX_BINARY_BYTES = 2 * 1024**3
MAX_OUTPUT_BYTES = 1024**2


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024**2), b""):
            result.update(chunk)
    return result.hexdigest()


def child_limits():
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    resource.setrlimit(resource.RLIMIT_FSIZE, (MAX_OUTPUT_BYTES, MAX_OUTPUT_BYTES))


def command(argv, output, timeout=30):
    """No shell, core dump, unlimited output, or surviving timed-out process group."""
    with output.open("xb") as stream:
        try:
            child = subprocess.Popen(
                argv, stdout=stream, stderr=subprocess.STDOUT,
                start_new_session=True, preexec_fn=child_limits,
            )
        except OSError as error:
            stream.write(f"tool unavailable: {type(error).__name__}\n".encode())
            return 127
        try:
            status = child.wait(timeout=timeout)
            return 125 if output.stat().st_size >= MAX_OUTPUT_BYTES else status
        except subprocess.TimeoutExpired:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait()
            return 124


def capture(binary, evidence, phase):
    binary = binary.resolve(strict=True)
    size = binary.stat().st_size
    if not binary.is_file() or not 0 < size <= MAX_BINARY_BYTES:
        raise ValueError("binary outside diagnostic size bound")
    root = evidence / phase
    root.mkdir(parents=True, exist_ok=False)
    sha = digest(binary)
    binaries = evidence / "binaries"
    binaries.mkdir(exist_ok=True)
    snapshot = binaries / f"{sha}.gz"
    if not snapshot.exists():
        with binary.open("rb") as source, snapshot.open("xb") as destination:
            with gzip.GzipFile(fileobj=destination, mode="wb", compresslevel=1, mtime=0) as archive:
                shutil.copyfileobj(source, archive, length=1024**2)

    details = {}
    for name, argv in (
        ("file", ["file", "-b", str(binary)]),
        ("elf-header", ["readelf", "-h", str(binary)]),
        ("elf-program-headers", ["readelf", "-l", str(binary)]),
        ("elf-dynamic", ["readelf", "-d", str(binary)]),
    ):
        details[name] = command(argv, root / f"{name}.txt")
    probe_exit = command([str(binary), "--version"], root / "version.txt")
    if probe_exit != 0:
        # Function names/locations only: no locals, arguments, registers, memory,
        # environment, or raw core dumps. This diagnostic rerun cannot pass a gate.
        details["backtrace"] = command([
            "gdb", "--batch", "--nx", "--quiet",
            "-iex", "set auto-load off",
            "-ex", "set pagination off", "-ex", "set startup-with-shell off",
            "-ex", "set disable-randomization off",
            "-ex", "set print frame-arguments none",
            "-ex", "set print entry-values no",
            "-ex", "run", "-ex", "thread apply all bt 30",
            "--args", str(binary), "--version",
        ], root / "backtrace.txt")
    report = {
        "phase": phase, "binary_sha256": sha, "binary_bytes": size,
        "probe_exit": probe_exit, "diagnostic_commands": details,
        "source_sha": os.environ.get("GITHUB_SHA", ""),
        "run_id": os.environ.get("GITHUB_RUN_ID", ""),
        "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT", ""),
    }
    (root / "result.json").write_text(json.dumps(report, sort_keys=True) + "\n")
    # Capture permits subsequent strip/staging comparisons. require_success is a
    # mandatory separate step before the unchanged formal package smoke.
    print(json.dumps(report, sort_keys=True))
    # Keep the essential diagnosis readable in Actions even if artifact export is
    # unavailable. These commands contain no environment, locals or memory dumps.
    for name in ("file", "elf-header", "elf-dynamic", "backtrace"):
        path = root / f"{name}.txt"
        if path.exists():
            data = path.read_bytes()
            print(f"--- {phase}: {name} ---")
            print(data[:32768].decode("utf-8", errors="replace"))
            if len(data) > 32768:
                print("[diagnostic log excerpt truncated; bounded full file retained]")


def require_success(evidence):
    results = [json.loads((evidence / phase / "result.json").read_text()) for phase in PHASES]
    for field, variable in (("source_sha", "GITHUB_SHA"), ("run_id", "GITHUB_RUN_ID"),
                            ("run_attempt", "GITHUB_RUN_ATTEMPT")):
        expected = os.environ.get(variable)
        if not expected or any(result[field] != expected for result in results):
            raise ValueError("diagnostic phases do not share this build subject")
    if any(result["phase"] != phase or result["probe_exit"] != 0
           for phase, result in zip(PHASES, results)):
        raise ValueError("a release startup probe failed; see retained diagnostics")
    if results[1]["binary_sha256"] != results[2]["binary_sha256"]:
        raise ValueError("staging changed the stripped runtime bytes")
    versions = [(evidence / phase / "version.txt").read_bytes() for phase in PHASES]
    if not versions[0].strip() or len(set(versions)) != 1:
        raise ValueError("release startup output changed across package construction")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", required=True, type=Path)
    parser.add_argument("--phase", choices=PHASES)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--require-success", action="store_true")
    args = parser.parse_args()
    if args.require_success:
        if args.phase is not None or args.binary is not None:
            parser.error("the gate does not execute a binary")
        require_success(args.evidence)
    else:
        if args.phase is None or args.binary is None:
            parser.error("capture requires both phase and binary")
        capture(args.binary, args.evidence, args.phase)


if __name__ == "__main__":
    main()
