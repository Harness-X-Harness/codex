#!/usr/bin/env python3
"""Offline controls for the two exact-stock lint exceptions; no Rust compilation."""
import importlib.util
import pathlib
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("stock_lint", HERE / "grok-stock-lint.py")
lint = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(lint)


class StockLintTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)
        self.originals = {}
        for name, _, _, _, _ in lint.EXCEPTIONS:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(HERE.parents[1] / name, path)
            self.originals[name] = path.read_bytes()

    def assert_restored(self):
        for name, data in self.originals.items():
            self.assertEqual((self.root / name).read_bytes(), data)

    def test_exact_temporary_edits_and_command_preserved(self):
        command = ["just", "clippy", "--locked", "--", "--cap-lints=forbid"]
        def invoke(actual):
            self.assertEqual(actual, command)
            for name, _, _, _, changed_sha in lint.EXCEPTIONS:
                self.assertEqual(lint.blob((self.root / name).read_bytes()), changed_sha)
            return subprocess.CompletedProcess(actual, 0)
        with patch.object(lint.subprocess, "run", side_effect=invoke):
            self.assertEqual(lint.run(self.root, command), 0)
        self.assert_restored()

    def test_lint_failure_preserved_and_sources_restored(self):
        with patch.object(lint.subprocess, "run", return_value=subprocess.CompletedProcess([], 101)):
            self.assertEqual(lint.run(self.root, ["just", "fix"]), 101)
        self.assert_restored()

    def test_command_exception_restores_sources(self):
        with patch.object(lint.subprocess, "run", side_effect=OSError("missing command")):
            with self.assertRaises(OSError):
                lint.run(self.root, ["missing"])
        self.assert_restored()

    def test_changed_stock_input_fails_before_command(self):
        path = self.root / lint.EXCEPTIONS[1][0]
        path.write_bytes(path.read_bytes() + b"// future stock\n")
        with patch.object(lint.subprocess, "run") as command:
            with self.assertRaisesRegex(RuntimeError, "reassessment"):
                lint.run(self.root, ["just", "fix"])
            command.assert_not_called()
        first = lint.EXCEPTIONS[0][0]
        self.assertEqual((self.root / first).read_bytes(), self.originals[first])

    def test_unexpected_same_file_edit_fails_and_is_not_erased(self):
        path = self.root / lint.EXCEPTIONS[0][0]
        def invoke(command):
            path.write_bytes(path.read_bytes() + b"// unexpected\n")
            return subprocess.CompletedProcess(command, 0)
        with patch.object(lint.subprocess, "run", side_effect=invoke):
            with self.assertRaisesRegex(RuntimeError, "unexpected lint edits"):
                lint.run(self.root, ["just", "fix"])
        self.assertTrue(path.read_bytes().endswith(b"// unexpected\n"))
        other = lint.EXCEPTIONS[1][0]
        self.assertEqual((self.root / other).read_bytes(), self.originals[other])

    def test_missing_file_fails_but_other_stock_file_is_restored(self):
        path = self.root / lint.EXCEPTIONS[0][0]
        def invoke(command):
            path.unlink()
            return subprocess.CompletedProcess(command, 0)
        with patch.object(lint.subprocess, "run", side_effect=invoke):
            with self.assertRaisesRegex(RuntimeError, "unexpected lint edits"):
                lint.run(self.root, ["just", "fix"])
        self.assertFalse(path.exists())
        other = lint.EXCEPTIONS[1][0]
        self.assertEqual((self.root / other).read_bytes(), self.originals[other])

    def test_unrelated_dirty_file_survives_for_existing_clean_gate(self):
        path = self.root / "unrelated.rs"
        def invoke(command):
            path.write_text("unexpected edit\n")
            return subprocess.CompletedProcess(command, 0)
        with patch.object(lint.subprocess, "run", side_effect=invoke):
            self.assertEqual(lint.run(self.root, ["just", "fix"]), 0)
        self.assertEqual(path.read_text(), "unexpected edit\n")
        self.assert_restored()


if __name__ == "__main__":
    unittest.main()
