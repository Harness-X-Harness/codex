#!/usr/bin/env python3
"""Print bounded, numeric build resource samples; never read argv or environment."""

import json
import os
from pathlib import Path
import signal
import sys
import threading
import time

PROCESSES = frozenset(("Runner.Listener", "Runner.Worker", "rustc", "cargo", "sccache",
                       "ld", "ld.lld", "rust-lld", "lld", "collect2", "cc", "gcc", "zig",
                       "lto1", "lto-wrapper", "x86_64-linux-mu", "x86_64-linux-gn"))
EVENTS = ("low", "high", "max", "oom", "oom_kill", "oom_group_kill")


def number(path):
    try:
        value = path.read_text().strip()
        return int(value) if value.isdecimal() else None
    except OSError:
        return None


def sample(proc=Path("/proc"), cgroup=Path("/sys/fs/cgroup"), disk=Path(".")):
    result = {"monotonic_s": int(time.monotonic())}
    # Only parsed numbers and fixed names are emitted, including on read errors.
    for line in (proc / "meminfo").read_text().splitlines():
        key, _, value = line.partition(":")
        if key in ("MemAvailable", "MemTotal"):
            result[key + "_kib"] = int(value.split()[0])
    fs = os.statvfs(disk)
    result["disk_available_bytes"] = fs.f_bavail * fs.f_frsize
    for line in (proc / "self/cgroup").read_text().splitlines():
        if line.startswith("0::/"):
            relative = Path(line[4:])
            if ".." not in relative.parts:
                group = cgroup / relative
                result["cgroup"] = {key: number(group / ("memory." + key))
                                    for key in ("current", "peak", "max")}
                try:
                    events = dict(line.split() for line in (group / "memory.events").read_text().splitlines())
                except OSError:
                    events = {}
                result["cgroup"]["events"] = {key: int(events[key]) for key in EVENTS
                                                  if events.get(key, "").isdecimal()}
            break
    processes = []
    for entry in proc.iterdir():
        if not entry.name.isdecimal():
            continue
        try:
            name = (entry / "comm").read_text().strip()
            if name not in PROCESSES:
                continue
            rss = None
            for line in (entry / "status").read_text().splitlines():
                if line.startswith("VmRSS:"):
                    rss = int(line.split()[1])
            processes.append({"pid": int(entry.name), "name": name, "rss_kib": rss})
        except (OSError, ValueError):
            continue  # A process can exit between the procfs reads.
    # Cap output even if a broken build leaves thousands of children behind.
    result["process_count"] = len(processes)
    result["processes"] = sorted(processes, key=lambda p: (-(p["rss_kib"] or 0), p["pid"]))[:32]
    return result


def main():
    stopped = threading.Event()
    signal.signal(signal.SIGTERM, lambda *_: stopped.set())
    parent = int(sys.argv[1])
    while os.getppid() == parent and not stopped.is_set():
        try:
            print("grok-build-resources " + json.dumps(sample(), sort_keys=True), flush=True)
        except Exception:
            # Exceptions can contain paths; never print their text or traceback.
            print("grok-build-resources sample_failed", file=sys.stderr, flush=True)
            return 1
        stopped.wait(10)
    return 0


if __name__ == "__main__":
    sys.exit(main())
