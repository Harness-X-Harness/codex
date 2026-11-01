#!/usr/bin/env python3
"""Offline safety and workflow ownership checks; never remove image SDKs."""

import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import prepare_linux_disk as prepare
from select_artifact import is_binary_input


class PrepareLinuxDisk(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.paths = tuple(self.root / name for name in ("android", "dotnet", "ghcup"))
        for path in self.paths:
            path.mkdir()
        self.environment = {
            "GITHUB_ACTIONS": "true",
            "RUNNER_ENVIRONMENT": "github-hosted",
            "RUNNER_OS": "Linux",
            "TARGET": "x86_64-unknown-linux-musl",
            "CARGO_TARGET_DIR": str(self.root),
        }

    def invoke(self, *, failure=None, output_failure=None):
        out, err = io.StringIO(), io.StringIO()

        def capture(descriptor, data):
            if output_failure is not None:
                raise output_failure
            (err if descriptor == 2 else out).write(data.decode())
            return len(data)

        with (
            mock.patch.dict(os.environ, self.environment, clear=True),
            mock.patch.object(prepare, "IMAGE_SDKS", self.paths),
            mock.patch.object(prepare.subprocess, "run", side_effect=failure) as command,
            mock.patch.object(prepare.os, "write", side_effect=capture),
        ):
            status = prepare.main()
        return status, command, out.getvalue(), err.getvalue()

    def test_fixed_image_allowlist_has_no_workspace_or_build_cache(self):
        self.assertEqual(
            prepare.IMAGE_SDKS,
            tuple(map(Path, ("/usr/local/lib/android", "/usr/share/dotnet", "/usr/local/.ghcup"))),
        )

    def test_only_explicit_hosted_linux_package_may_remove_sdks(self):
        for key, value in (
            ("GITHUB_ACTIONS", "false"),
            ("RUNNER_ENVIRONMENT", "self-hosted"),
            ("RUNNER_OS", "macOS"),
            ("RUNNER_OS", "Windows"),
            ("TARGET", "aarch64-apple-darwin"),
        ):
            with self.subTest(key=key, value=value):
                original = self.environment[key]
                self.environment[key] = value
                status, command, _, _ = self.invoke()
                self.assertEqual(status, 0)
                command.assert_not_called()
                self.environment[key] = original
        for key in ("GITHUB_ACTIONS", "RUNNER_ENVIRONMENT", "RUNNER_OS", "TARGET"):
            with self.subTest(missing=key):
                original = self.environment.pop(key)
                status, command, _, _ = self.invoke()
                self.assertEqual(status, 0)
                command.assert_not_called()
                self.environment[key] = original

    def test_removes_only_present_roots_and_measures_target_filesystem(self):
        self.paths[-1].rmdir()
        with mock.patch.object(prepare.shutil, "disk_usage") as usage:
            usage.return_value.free = 123
            status, command, out, err = self.invoke()
        self.assertEqual((status, err), (0, ""))
        self.assertEqual(
            command.call_args_list,
            [
                mock.call(["sudo", "-n", "rm", "-rf", "--one-file-system", "--preserve-root=all", "--", str(path)], check=True)
                for path in self.paths[:-1]
            ],
        )
        self.assertEqual(usage.call_args_list, [mock.call(str(self.root))] * 2)
        self.assertEqual(
            [json.loads(line.removeprefix("grok-build-disk ")) for line in out.splitlines()],
            [{"phase": phase, "available_bytes": 123} for phase in ("before", "after")],
        )

    def test_symlink_root_is_rejected_before_any_deletion(self):
        self.paths[1].rmdir()
        self.paths[1].symlink_to(self.paths[0], target_is_directory=True)
        status, command, _, err = self.invoke()
        self.assertEqual(status, 1)
        command.assert_not_called()
        self.assertIn("unsafe_image_sdk_path", err)

    def test_redirected_ancestor_is_rejected_before_any_deletion(self):
        alias = self.root / "alias"
        alias.symlink_to(self.root, target_is_directory=True)
        self.paths = (self.paths[0], alias / "dotnet")
        status, command, _, _ = self.invoke()
        self.assertEqual(status, 1)
        command.assert_not_called()

    def test_non_directory_is_rejected_before_any_deletion(self):
        self.paths[1].rmdir()
        self.paths[1].write_text("not an image SDK directory")
        status, command, _, _ = self.invoke()
        self.assertEqual(status, 1)
        command.assert_not_called()

    def test_cleanup_failure_stops_removal_but_keeps_final_disk_diagnostic(self):
        status, command, out, err = self.invoke(failure=subprocess.CalledProcessError(1, "sudo"))
        self.assertEqual(status, 1)
        self.assertEqual(command.call_count, 1)
        self.assertIn('"phase": "after"', out)
        self.assertIn("image_sdk_cleanup_failed", err)

    def test_partial_cleanup_failure_stops_before_the_third_root(self):
        status, command, out, err = self.invoke(
            failure=[None, subprocess.CalledProcessError(1, "sudo")]
        )
        self.assertEqual(status, 1)
        self.assertEqual(command.call_count, 2)
        self.assertIn('"phase": "after"', out)
        self.assertIn("image_sdk_cleanup_failed", err)

    def test_missing_measurement_does_not_change_cleanup_result(self):
        with mock.patch.object(prepare.shutil, "disk_usage", side_effect=OSError):
            status, command, out, _ = self.invoke()
        self.assertEqual(status, 0)
        self.assertEqual(command.call_count, 3)
        self.assertEqual(out.count('"available_bytes": null'), 2)

    def test_unavailable_diagnostic_output_does_not_change_cleanup_result(self):
        status, command, _, _ = self.invoke(output_failure=BrokenPipeError())
        self.assertEqual(status, 0)
        self.assertEqual(command.call_count, 3)

    def test_unavailable_output_preserves_skipped_and_failed_cleanup_results(self):
        status, command, _, _ = self.invoke(
            failure=subprocess.CalledProcessError(1, "sudo"), output_failure=BrokenPipeError()
        )
        self.assertEqual(status, 1)
        self.assertEqual(command.call_count, 1)
        self.environment["RUNNER_ENVIRONMENT"] = "self-hosted"
        status, command, _, _ = self.invoke(output_failure=BrokenPipeError())
        self.assertEqual(status, 0)
        command.assert_not_called()

    def test_closed_stdout_preserves_process_exit_for_ineligible_environment(self):
        read_fd, write_fd = os.pipe()
        os.close(read_fd)
        try:
            result = subprocess.run(
                [sys.executable, str(Path(prepare.__file__).resolve())],
                env={"GITHUB_ACTIONS": "false"},
                stdout=write_fd,
                stderr=subprocess.PIPE,
                check=False,
                timeout=10,
            )
        finally:
            os.close(write_fd)
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertEqual(result.stderr, b"")

    def test_action_and_offline_ci_own_the_helper(self):
        root = Path(__file__).resolve().parents[2]
        action = (root / ".github/actions/build-grok/action.yml").read_text()
        step = action.split("    - name: Reclaim unused hosted Linux image SDKs\n", 1)[1].split("    - ", 1)[0]
        self.assertIn("if: runner.os == 'Linux'", step)
        self.assertIn("TARGET: ${{ inputs.target }}", step)
        self.assertIn("python3 grok/package/prepare_linux_disk.py", step)
        self.assertLess(action.index("Reclaim unused hosted Linux image SDKs"), action.index("Prepare the Linux builder"))
        workflow = (root / ".github/workflows/grok-go.yml").read_text()
        self.assertIn("python3 grok/package/test_prepare_linux_disk.py -v", workflow)
        self.assertTrue(is_binary_input("grok/package/prepare_linux_disk.py"))


if __name__ == "__main__":
    unittest.main()
