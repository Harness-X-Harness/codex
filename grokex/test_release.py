import tempfile
import unittest
from pathlib import Path

from grokex import release

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


class ProfileTest(unittest.TestCase):
    def test_live_profile_rejects_catalog_and_child_model_overrides(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            profile = Path(temporary) / "config.toml"
            profile.write_text(
                """
model = "grok-4.6"
model_provider = "grok"

[agents]
default_subagent_model = "grok-4.5"

[model_providers.grok]
base_url = "https://grok.trustedtunnel.app/v1"
experimental_bearer_token = "secret"
requires_openai_auth = false
supports_websockets = false
wire_api = "grok_responses"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(SystemExit, "must not override the default child model"):
                release.verify_profile(profile, secret=True)

            profile.write_text(
                profile.read_text(encoding="utf-8").replace(
                    '[agents]\ndefault_subagent_model = "grok-4.5"\n',
                    'model_catalog_json = "custom-catalog.json"\n',
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(SystemExit, "release-bundled model catalog"):
                release.verify_profile(profile, secret=True)

    def test_public_profile_is_valid_and_token_free(self) -> None:
        release.verify_profile(REPOSITORY / release.DIST_ROOT / "config.toml.example", secret=False)


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
