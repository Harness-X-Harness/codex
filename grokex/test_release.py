import json
import subprocess
import tempfile
import unittest
from pathlib import Path

from grokex import release

REPOSITORY = Path(__file__).resolve().parent.parent


def git(repo: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=repo, check=True, capture_output=True, text=True
    ).stdout.strip()


def commit_all(repo: Path, message: str) -> str:
    git(repo, "add", "-A")
    git(
        repo,
        "-c",
        "user.name=Release Test",
        "-c",
        "user.email=release@example.invalid",
        "commit",
        "-q",
        "-m",
        message,
    )
    return git(repo, "rev-parse", "HEAD")


def write_raw_binaries(raw_root: Path) -> None:
    for target in release.TARGETS:
        raw = raw_root / target
        raw.mkdir(parents=True)
        suffix = ".exe" if "windows" in target else ""
        (raw / f"codex{suffix}").write_bytes(b"codex")
        (raw / f"codex-code-mode-host{suffix}").write_bytes(b"host")
        if "linux" in target:
            (raw / "bwrap").write_bytes(b"bwrap")


def write_evidence(evidence_dir: Path, mode: str = "release", status: str = "completed") -> None:
    evidence_dir.mkdir(parents=True, exist_ok=True)
    for scenario, contract in release.SCENARIOS.items():
        (evidence_dir / f"LIVE_EVIDENCE-{scenario}.json").write_text(
            json.dumps(
                {
                    "mode": mode,
                    "scenario": scenario,
                    "status": status,
                    "story": contract["story"],
                    "turn_durations_seconds": [1.5],
                }
            ),
            encoding="utf-8",
        )


class ProductIdentityTest(unittest.TestCase):
    def test_identity_derives_from_release_source(self) -> None:
        values = release.identity(REPOSITORY, "HEAD")
        self.assertEqual(values["release_tag"], f"grokex-v{release.VERSION}")
        self.assertEqual(values["upstream_tag"], f"rust-v{release.VERSION}")
        self.assertEqual(len(values["product_tree"]), 64)

    def test_product_tree_ignores_validation_paths_and_follows_product_paths(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            repo = Path(scratch)
            git(repo, "init", "-q", "-b", "main")
            for path in release.PRODUCT_PATHS:
                target = repo / path
                if Path(path).suffix:
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_text("{}\n", encoding="utf-8")
                else:
                    target.mkdir(parents=True, exist_ok=True)
                    (target / "keep").write_text("v1\n", encoding="utf-8")
            (repo / "grokex" / "validator").mkdir(parents=True)
            (repo / "grokex" / "validator" / "main.go").write_text("package main\n", encoding="utf-8")
            base = commit_all(repo, "base")
            baseline = release.product_tree(repo, base)

            (repo / "grokex" / "validator" / "main.go").write_text("package main // fixed\n", encoding="utf-8")
            (repo / "README.md").write_text("docs\n", encoding="utf-8")
            validation_only = commit_all(repo, "validator fix")
            self.assertEqual(release.product_tree(repo, validation_only), baseline)

            (repo / "codex-rs" / "keep").write_text("v2\n", encoding="utf-8")
            product_change = commit_all(repo, "product change")
            self.assertNotEqual(release.product_tree(repo, product_change), baseline)

            with self.assertRaisesRegex(SystemExit, "product path .* is missing"):
                release.product_tree(repo, f"{base}^{{tree}}:nope")


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


class ReleaseEnvelopeTest(unittest.TestCase):
    def test_packages_assembles_and_verifies_release_assets(self) -> None:
        tree = "a" * 64
        with tempfile.TemporaryDirectory() as temporary:
            scratch = Path(temporary)
            write_raw_binaries(scratch / "raw")
            release.package(scratch / "raw", scratch / "archives", REPOSITORY, tree, "b" * 40)
            release.verify_archives(scratch / "archives", tree)
            with self.assertRaisesRegex(SystemExit, "provenance mismatch"):
                release.verify_archives(scratch / "archives", "c" * 64)

            # An archive built from another commit with the same product tree is
            # the same product: verification compares the tree, not the commit.
            release.package(
                scratch / "raw",
                scratch / "archives-later",
                REPOSITORY,
                tree,
                "d" * 40,
                (release.LIVE_TARGET,),
            )
            release.verify_archives(scratch / "archives-later", tree, (release.LIVE_TARGET,))

            write_evidence(scratch / "evidence")
            release.assemble(
                scratch / "archives",
                scratch / "evidence",
                scratch / "assets",
                REPOSITORY,
                "e" * 40,
                tree,
                "12345",
            )
            release.verify_assets(scratch / "assets", "e" * 40, tree, "12345")

            manifest = json.loads((scratch / "assets" / "RELEASE.json").read_text(encoding="utf-8"))
            self.assertEqual(
                manifest,
                {
                    "archives": [release.archive_name(target) for target in release.TARGETS],
                    "live_archive": release.archive_name(release.LIVE_TARGET),
                    "product_tree": tree,
                    "release_run": "12345",
                    "scenarios": {
                        scenario: {
                            "status": "completed",
                            "story": contract["story"],
                            "turn_durations_seconds": [1.5],
                        }
                        for scenario, contract in release.SCENARIOS.items()
                    },
                    "source_sha": "e" * 40,
                    "tag": release.TAG,
                    "upstream_commit": release.UPSTREAM_COMMIT,
                    "version": release.VERSION,
                },
            )
            asset_names = {path.name for path in (scratch / "assets").iterdir()}
            self.assertEqual(
                asset_names,
                {release.archive_name(target) for target in release.TARGETS}
                | set(release.RELEASE_ASSET_DIST_FILES)
                | {"RELEASE.json", "SHA256SUMS"},
            )

            with self.assertRaisesRegex(SystemExit, "release manifest mismatch"):
                release.verify_assets(scratch / "assets", "f" * 40, tree, "12345")
            (scratch / "assets" / "SHA256SUMS").write_text("0" * 64 + "  RELEASE.json\n", encoding="utf-8")
            with self.assertRaisesRegex(SystemExit, "checksum manifest file set mismatch"):
                release.verify_assets(scratch / "assets", "e" * 40, tree, "12345")

    def test_assemble_requires_every_scenario_as_a_completed_release_proof(self) -> None:
        tree = "a" * 64
        with tempfile.TemporaryDirectory() as temporary:
            scratch = Path(temporary)
            write_raw_binaries(scratch / "raw")
            release.package(scratch / "raw", scratch / "archives", REPOSITORY, tree, "b" * 40)

            write_evidence(scratch / "observation", mode="observation")
            with self.assertRaisesRegex(SystemExit, "not a completed release proof"):
                release.assemble(scratch / "archives", scratch / "observation", scratch / "out1", REPOSITORY, "e" * 40, tree, "1")

            write_evidence(scratch / "partial")
            (scratch / "partial" / "LIVE_EVIDENCE-basic-exact-reply.json").unlink()
            with self.assertRaisesRegex(SystemExit, "live evidence is incomplete"):
                release.assemble(scratch / "archives", scratch / "partial", scratch / "out2", REPOSITORY, "e" * 40, tree, "1")


if __name__ == "__main__":
    unittest.main()
