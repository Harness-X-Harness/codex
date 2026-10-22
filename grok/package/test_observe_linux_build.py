#!/usr/bin/env python3
"""Offline resource privacy, build exit-status and observer-lifetime checks."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

import observe_linux_build as observer


class BuildResources(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.proc = self.root / "proc"
        (self.proc / "self").mkdir(parents=True)
        (self.proc / "meminfo").write_text("MemTotal: 1234 kB\nMemAvailable: 567 kB\nSecret: NEVER_EMIT\n")
        (self.proc / "self/cgroup").write_text("0::/build\n")
        self.cgroup = self.root / "cgroup"
        group = self.cgroup / "build"
        group.mkdir(parents=True)
        for key, value in (("current", "123"), ("peak", "456"), ("max", "max")):
            (group / ("memory." + key)).write_text(value)
        (group / "memory.events").write_text("oom 2\noom_kill 1\nsecret NEVER_EMIT\n")

    def process(self, pid, name, rss):
        path = self.proc / str(pid)
        path.mkdir()
        (path / "comm").write_text(name)
        (path / "status").write_text(f"VmRSS:\t{rss} kB\nSecret: NEVER_EMIT\n")
        (path / "cmdline").write_text("NEVER_EMIT")
        (path / "environ").write_text("NEVER_EMIT")

    def sample(self):
        return observer.sample(self.proc, self.cgroup, self.root)

    def test_allowlists_only_numbers_and_known_process_names(self):
        self.process(12, "rustc", 100)
        self.process(13, "Runner.Worker", 200)
        self.process(14, "NEVER_EMIT", 99999)
        result = self.sample()
        self.assertEqual(result["MemAvailable_kib"], 567)
        self.assertEqual(result["cgroup"], {"current": 123, "peak": 456, "max": None,
                                           "events": {"oom": 2, "oom_kill": 1}})
        self.assertEqual(result["processes"], [{"pid": 13, "name": "Runner.Worker", "rss_kib": 200},
                                               {"pid": 12, "name": "rustc", "rss_kib": 100}])
        self.assertNotIn("NEVER_EMIT", json.dumps(result))
        self.assertGreaterEqual(result["disk_available_bytes"], 0)

    def test_process_output_is_bounded_and_largest_first(self):
        for pid in range(1, 41):
            self.process(pid, "rustc", pid)
        result = self.sample()
        self.assertEqual(result["process_count"], 40)
        self.assertEqual(len(result["processes"]), 32)
        self.assertEqual(result["processes"][0]["pid"], 40)

    def test_unavailable_cgroup_metrics_are_explicitly_unknown(self):
        shutil.rmtree(self.cgroup)
        self.assertEqual(self.sample()["cgroup"], {"current": None, "peak": None, "max": None, "events": {}})

    def test_parent_path_cannot_escape_cgroup_root(self):
        (self.proc / "self/cgroup").write_text("0::/../../NEVER_EMIT\n")
        self.assertNotIn("cgroup", self.sample())

    def invoke(self, command, observer_status=None):
        # Substitute only the observer, keeping the production shell unchanged.
        script = Path(observer.__file__).with_suffix(".sh")
        shutil.copyfile(script, self.root / script.name)
        code = '''import os, signal, time
from pathlib import Path
Path("observer.pid").write_text(str(os.getpid()))
signal.signal(signal.SIGTERM, lambda *_: exit(0))
while True: time.sleep(0.01)
'''
        if observer_status is not None:
            code = f"exit({observer_status})\n"
        (self.root / "observe_linux_build.py").write_text(code)
        result = subprocess.run(["bash", str(self.root / script.name), "bash", "-c", command],
                                cwd=self.root, capture_output=True, text=True, timeout=10)
        pidfile = self.root / "observer.pid"
        if pidfile.exists():
            with self.assertRaises(ProcessLookupError):
                os.kill(int(pidfile.read_text()), 0)
        return result

    def test_observer_exits_if_parent_was_already_lost(self):
        result = subprocess.run(["python3", observer.__file__, "0"],
                                capture_output=True, text=True, timeout=2)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")

    def test_success_reaps_observer(self):
        self.assertEqual(self.invoke("sleep 0.2; exit 0").returncode, 0)

    def test_failure_reaps_observer_and_preserves_compiler_status(self):
        self.assertEqual(self.invoke("sleep 0.2; exit 42").returncode, 42)

    def test_signalled_command_preserves_status_and_reaps_observer(self):
        self.assertEqual(self.invoke("sleep 0.2; kill -TERM $$").returncode, 143)

    def test_observer_failure_is_reported_without_changing_compiler_status(self):
        result = self.invoke("sleep 0.2; exit 0", observer_status=7)
        self.assertEqual(result.returncode, 0)
        self.assertIn("observer_failed", result.stderr)

    def test_observer_failure_does_not_replace_compiler_failure(self):
        self.assertEqual(self.invoke("sleep 0.2; exit 42", observer_status=7).returncode, 42)


if __name__ == "__main__":
    unittest.main()
