#!/usr/bin/env python3
"""Exercise actual startup probes, retained bytes and the mandatory failure gate."""

import gzip
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

import diagnose_linux_binary as diagnostics


SCRIPT = Path(diagnostics.__file__).resolve()


class LinuxStartupDiagnostics(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.evidence = self.root / "evidence"
        self.env = dict(os.environ, GITHUB_SHA="a" * 40, GITHUB_RUN_ID="123", GITHUB_RUN_ATTEMPT="1")

    def binary(self, name="runtime", version="codex-cli 0.158.0", status=0):
        path = self.root / name
        path.write_text(f"#!/bin/sh\nprintf '%s\\n' '{version}'\nexit {status}\n")
        path.chmod(0o700)
        return path

    def invoke(self, *args, env=None):
        return subprocess.run(
            [sys.executable, str(SCRIPT), "--evidence", str(self.evidence), *args],
            env=self.env if env is None else env, capture_output=True, text=True, timeout=40,
        )

    def capture(self, phase, binary):
        result = self.invoke("--phase", phase, "--binary", str(binary))
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads((self.evidence / phase / "result.json").read_text())

    def test_complete_same_build_preserves_exact_bytes_and_passes(self):
        binary = self.binary()
        for phase in diagnostics.PHASES:
            report = self.capture(phase, binary)
            self.assertEqual(report["probe_exit"], 0)
        snapshots = list((self.evidence / "binaries").glob("*.gz"))
        self.assertEqual(len(snapshots), 1)
        self.assertEqual(gzip.decompress(snapshots[0].read_bytes()), binary.read_bytes())
        result = self.invoke("--require-success")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_early_failure_keeps_later_comparisons_and_cannot_pass(self):
        bad = self.binary("before", status=139)
        report = self.capture("before-strip", bad)
        self.assertEqual(report["probe_exit"], 139)
        good = self.binary()
        for phase in ("after-strip", "staged"):
            self.assertEqual(self.capture(phase, good)["probe_exit"], 0)
        result = self.invoke("--require-success")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("startup probe failed", result.stderr)
        self.assertTrue((self.evidence / "staged/result.json").is_file())

    def test_real_signal_is_retained_and_gate_rejects(self):
        binary = self.root / "runtime"
        binary.write_text("#!/bin/sh\nkill -TERM $$\n")
        binary.chmod(0o700)
        for phase in diagnostics.PHASES:
            self.assertEqual(self.capture(phase, binary)["probe_exit"], -15)
        self.assertNotEqual(self.invoke("--require-success").returncode, 0)

    def test_staging_mutation_with_same_version_is_rejected(self):
        binary = self.binary()
        for phase in ("before-strip", "after-strip"):
            self.capture(phase, binary)
        changed = self.binary("staged")
        changed.write_text(changed.read_text() + "# changed bytes\n")
        self.capture("staged", changed)
        result = self.invoke("--require-success")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("staging changed", result.stderr)

    def test_strip_version_change_is_rejected(self):
        self.capture("before-strip", self.binary("before", version="codex-cli wrong"))
        binary = self.binary()
        for phase in ("after-strip", "staged"):
            self.capture(phase, binary)
        result = self.invoke("--require-success")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("output changed", result.stderr)

    def test_missing_phase_cannot_pass(self):
        self.capture("before-strip", self.binary())
        self.assertNotEqual(self.invoke("--require-success").returncode, 0)

    def test_other_run_attempt_cannot_pass(self):
        binary = self.binary()
        for phase in diagnostics.PHASES:
            self.capture(phase, binary)
        result = self.invoke("--require-success", env=dict(self.env, GITHUB_RUN_ATTEMPT="2"))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("build subject", result.stderr)

    def test_repeat_phase_does_not_overwrite_failed_evidence(self):
        report = self.capture("before-strip", self.binary("bad", status=1))
        result = self.invoke("--phase", "before-strip", "--binary", str(self.binary()))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(json.loads((self.evidence / "before-strip/result.json").read_text()), report)

    def test_timeout_kills_child_process_group(self):
        marker = self.root / "survived"
        program = self.root / "delayed"
        program.write_text(f"#!/bin/sh\nsleep 0.3\ntouch '{marker}'\n")
        program.chmod(0o700)
        status = diagnostics.command([str(program)], self.root / "timeout.txt", timeout=0.03)
        self.assertEqual(status, 124)
        time.sleep(0.4)
        self.assertFalse(marker.exists())

    def test_output_limit_cannot_become_success(self):
        output = self.root / "bounded.txt"
        status = diagnostics.command([sys.executable, "-c", "import os; os.write(1, b'x' * (2 * 1024**2))"], output)
        self.assertNotEqual(status, 0)
        self.assertLessEqual(output.stat().st_size, diagnostics.MAX_OUTPUT_BYTES)


if __name__ == "__main__":
    unittest.main()
