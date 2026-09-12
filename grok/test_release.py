import tempfile
import unittest
from pathlib import Path

from grok import release

REPOSITORY = Path(__file__).resolve().parent.parent
VERSION = "1.2.3"
SHA = "b" * 40


def write_raw_binaries(raw_root: Path) -> None:
    for target in release.TARGETS:
        raw = raw_root / target
        raw.mkdir(parents=True)
        suffix = ".exe" if "windows" in target else ""
        (raw / f"codex{suffix}").write_bytes(b"codex")
        (raw / f"codex-code-mode-host{suffix}").write_bytes(b"host")
        if "linux" in target:
            (raw / "bwrap").write_bytes(b"bwrap")


class ArchiveTest(unittest.TestCase):
    def test_packages_and_verifies_archives(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            scratch = Path(temporary)
            write_raw_binaries(scratch / "raw")
            release.package(
                scratch / "raw",
                scratch / "archives",
                REPOSITORY,
                VERSION,
                SHA,
            )
            release.verify_archives(scratch / "archives", VERSION, SHA)
            with self.assertRaisesRegex(SystemExit, "provenance mismatch"):
                release.verify_archives(scratch / "archives", VERSION, "c" * 40)
            checksums = release.write_checksums(scratch / "archives")
            self.assertTrue(checksums.is_file())
            names = {path.name for path in (scratch / "archives").glob("*.tar.gz")}
            self.assertEqual(
                names,
                {release.archive_name(VERSION, target) for target in release.TARGETS},
            )
