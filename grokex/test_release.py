import json
import tempfile
import unittest
from pathlib import Path

from grokex import release


ARCHIVE_DIGEST = "dda18a0e21ae47c53b4309434cbc02ae8bf764fa83a6defbb719431242722aa7"
CONTRACT_DIGEST = release.sha256(release.LIVE_CONTRACTS_PATH)


def scenario_evidence(archive_name: str, run_id: str) -> dict[str, dict[str, object]]:
    common = {
        "archive": archive_name,
        "archive_sha256": ARCHIVE_DIGEST,
        "catalog": "release-bundled",
        "contract_sha256": CONTRACT_DIGEST,
        "mode": "release",
        "model": "grok-4.6",
        "provider": "grok",
        "runner_turn_submission_count": 1,
        "source_sha": "source-sha",
        "status": "completed",
        "validation_run": run_id,
        "validator_sha": "validator-sha",
    }
    return {
        "basic-exact-reply": {
            **common,
            "response_assertion": "nonempty_agent_message",
            "scenario": "basic-exact-reply",
            "story": "grokex-provider-profile-startup",
        },
        "encrypted-reasoning-tool-continuation": {
            **common,
            "encrypted_reasoning_observed": True,
            "history_response_assertion": "exact_match",
            "response_assertion": "exact_match",
            "runner_turn_submission_count": 2,
            "same_thread_history": "completed",
            "scenario": "encrypted-reasoning-tool-continuation",
            "story": "grokex-encrypted-reasoning-history-continuation",
            "tool_continuation": "completed",
            "tool_request_count": 4,
        },
        "ultra-full-history-collaboration": {
            **common,
            "child_completion": "completed",
            "child_count": 3,
            "child_model_evidence": "parent_model_default_spawn_and_stock_inheritance",
            "child_model_verified": True,
            "child_parent_link_verified": True,
            "child_provider_binding": "grok/grok-4.6",
            "child_provider_verified": True,
            "child_response_assertion": "canonical_uuid_v4",
            "default_full_history": "completed",
            "evidence_source": "public_snapshot_and_stream",
            "explicit_fork_spawn_count": 1,
            "failed_collaboration_tool_count": 2,
            "missing_spawn_identity_count": 0,
            "multi_agent_version": "v2",
            "parent_completion": "completed",
            "provider_response_count": 9,
            "reasoning_effort": "ultra",
            "response_assertion": "child_echo_match",
            "runner_turn_submission_count": 1,
            "scenario": "ultra-full-history-collaboration",
            "story": "grokex-provider-binding-lifecycle",
            "spawn_count": 2,
            "unexpected_collaboration_tool_count": 1,
            "wait_count": 3,
            "result_delivery": "completed",
            "result_delivery_verified": True,
        },
        "image-generation-history-edit": {
            **common,
            "edit_agent_reply_seen": True,
            "edit_artifact_extension": ".webp",
            "edit_artifact_match": True,
            "edit_completion": "completed",
            "edit_image_decodable": True,
            "edit_image_mime": "image/webp",
            "generation_agent_reply_seen": True,
            "generation_artifact_extension": ".png",
            "generation_artifact_match": True,
            "generation_completion": "completed",
            "generation_image_decodable": True,
            "generation_image_mime": "image/png",
            "history_arguments_verified": True,
            "image_items_completed": 2,
            "image_items_failed": 1,
            "runner_turn_submission_count": 2,
            "same_thread": True,
            "scenario": "image-generation-history-edit",
            "story": "grokex-image-generation-history-edit",
        },
    }


def expected_manifest_scenarios(run_ids: dict[str, str]) -> dict[str, dict[str, object]]:
    scenarios: dict[str, dict[str, object]] = {}
    for scenario, assertions in release.LIVE_SCENARIO_ASSERTIONS.items():
        if scenario not in run_ids:
            continue
        evidence = scenario_evidence("unused", run_ids[scenario])[scenario]
        diagnostics = {
            key: evidence[key] for key in release.LIVE_SCENARIO_DIAGNOSTICS[scenario]
        }
        codec = (
            {
                key: evidence[key]
                for key in (
                    "generation_image_mime",
                    "generation_artifact_extension",
                    "edit_image_mime",
                    "edit_artifact_extension",
                )
            }
            if scenario == "image-generation-history-edit"
            else {}
        )
        scenarios[scenario] = {
            **assertions,
            **codec,
            **diagnostics,
            "story": release.STORY_BY_SCENARIO[scenario],
            "validation_run": run_ids[scenario],
        }
    return scenarios


class ReleaseIdentityTest(unittest.TestCase):
    def test_identity_derives_from_release_source(self) -> None:
        self.assertEqual(release.TAG, f"grokex-v{release.VERSION}")
        self.assertEqual(release.UPSTREAM_TAG, f"rust-v{release.VERSION}")
        self.assertEqual(
            release.archive_name("x86_64-unknown-linux-musl"),
            f"{release.TAG}-x86_64-unknown-linux-musl.tar.gz",
        )

    def test_required_scenarios_follow_seam_map(self) -> None:
        every_scenario = list(release.LIVE_CONTRACTS["scenarios"])
        cases = {
            "unknown base requires everything": (None, every_scenario),
            "documentation only keeps the always scenario": (
                ["grokex/INSTALL.md"],
                ["basic-exact-reply"],
            ),
            "image seam adds the image scenario": (
                ["codex-rs/ext/image-generation/src/tool.rs"],
                ["basic-exact-reply", "image-generation-history-edit"],
            ),
            "shared provider seam adds every seam scenario": (
                ["codex-rs/model-provider/src/grok_provider.rs"],
                every_scenario,
            ),
            "dependency lock requires everything": (["codex-rs/Cargo.lock"], every_scenario),
            "validator change requires everything": (
                ["grokex/live_smoke.py"],
                every_scenario,
            ),
        }
        for name, (changed, expected) in cases.items():
            with self.subTest(name):
                self.assertEqual(release.required_scenarios(changed), expected)

    def test_carrier_may_only_change_validation_paths(self) -> None:
        release.verify_carrier(
            [".github/workflows/grokex-live.yml", "grokex/live_smoke.py", "grokex/test_release.py"]
        )
        for product_path in (
            "grokex/config.toml.example",
            "grokex/install-grokex.sh",
            "codex-rs/model-provider/src/grok_provider.rs",
            ".github/actions/build-grokex/action.yml",
        ):
            with self.subTest(product_path):
                with self.assertRaisesRegex(SystemExit, "carrier changes product paths"):
                    release.verify_carrier(["grokex/live_smoke.py", product_path])


class LiveEvidenceTest(unittest.TestCase):
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

            with self.assertRaisesRegex(
                SystemExit, "must not override the default child model"
            ):
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

    def test_release_identity_rejects_rebound_live_evidence(self) -> None:
        evidence = {
            "archive": release.archive_name("x86_64-unknown-linux-musl"),
            "catalog": "release-bundled",
            "contract_sha256": CONTRACT_DIGEST,
            "model": "grok-4.6",
            "provider": "grok",
            "release_tag": release.TAG,
            "source_sha": "source-sha",
            "status": "completed",
            "validation_runs": ["run-a", "run-b"],
            "validator_sha": "source-sha",
        }
        release.verify_live_identity(evidence, "source-sha", "source-sha", ["run-b", "run-a"])

        for key in ("validator_sha", "catalog", "model", "contract_sha256", "release_tag"):
            with self.subTest(key=key):
                tampered = {**evidence, key: "different"}
                with self.assertRaises(SystemExit):
                    release.verify_live_identity(
                        tampered, "source-sha", "source-sha", ["run-a", "run-b"]
                    )
        with self.assertRaises(SystemExit):
            release.verify_live_identity(evidence, "source-sha", "source-sha", ["run-a"])

    def test_composes_manifest_across_live_runs(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / release.archive_name("x86_64-unknown-linux-musl")
            archive.write_bytes(b"candidate")
            evidence_dir = root / "evidence"
            evidence_dir.mkdir()
            first_run = scenario_evidence(archive.name, "run-a")
            second_run = scenario_evidence(archive.name, "run-b")
            for scenario in (
                "basic-exact-reply",
                "encrypted-reasoning-tool-continuation",
                "ultra-full-history-collaboration",
            ):
                (evidence_dir / f"a-{scenario}.json").write_text(
                    json.dumps(first_run[scenario]), encoding="utf-8"
                )
            (evidence_dir / "b-image.json").write_text(
                json.dumps(second_run["image-generation-history-edit"]), encoding="utf-8"
            )
            output = root / "LIVE_EVIDENCE.json"

            release.build_live_evidence(
                evidence_dir, archive, output, "source-sha", "validator-sha"
            )

            run_ids = {
                "basic-exact-reply": "run-a",
                "encrypted-reasoning-tool-continuation": "run-a",
                "ultra-full-history-collaboration": "run-a",
                "image-generation-history-edit": "run-b",
            }
            self.assertEqual(
                json.loads(output.read_text(encoding="utf-8")),
                {
                    "archive": archive.name,
                    "archive_sha256": ARCHIVE_DIGEST,
                    "catalog": "release-bundled",
                    "contract_sha256": CONTRACT_DIGEST,
                    "inherited_scenarios": {},
                    "model": "grok-4.6",
                    "provider": "grok",
                    "release_tag": release.TAG,
                    "required_scenarios": sorted(release.LIVE_SCENARIO_ASSERTIONS),
                    "runner_turn_submission_count": 6,
                    "scenarios": expected_manifest_scenarios(run_ids),
                    "source_sha": "source-sha",
                    "status": "completed",
                    "validation_runs": ["run-a", "run-b"],
                    "validator_sha": "validator-sha",
                },
            )

    def test_rejects_observation_and_foreign_contract_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / release.archive_name("x86_64-unknown-linux-musl")
            archive.write_bytes(b"candidate")
            for key, value in (("mode", "observation"), ("contract_sha256", "stale")):
                with self.subTest(key=key):
                    evidence_dir = root / f"evidence-{key}"
                    evidence_dir.mkdir()
                    for scenario, evidence in scenario_evidence(archive.name, "run-a").items():
                        tampered = {**evidence, key: value} if scenario == "basic-exact-reply" else evidence
                        (evidence_dir / f"{scenario}.json").write_text(
                            json.dumps(tampered), encoding="utf-8"
                        )
                    with self.assertRaisesRegex(SystemExit, "evidence mismatch"):
                        release.build_live_evidence(
                            evidence_dir,
                            archive,
                            root / f"{key}.json",
                            "source-sha",
                            "validator-sha",
                        )

    def test_inherits_unrequired_scenarios_from_the_published_base(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / release.archive_name("x86_64-unknown-linux-musl")
            archive.write_bytes(b"candidate")
            evidence_dir = root / "evidence"
            evidence_dir.mkdir()
            executed = scenario_evidence(archive.name, "run-c")
            for scenario in ("basic-exact-reply", "image-generation-history-edit"):
                (evidence_dir / f"{scenario}.json").write_text(
                    json.dumps(executed[scenario]), encoding="utf-8"
                )
            prior = {
                "archive_sha256": "prior-archive-digest",
                "release_tag": "grokex-v0.149.0",
                "scenarios": expected_manifest_scenarios(
                    {
                        scenario: "run-prior"
                        for scenario in release.LIVE_SCENARIO_ASSERTIONS
                    }
                ),
                "source_sha": "base-sha",
                "status": "completed",
                "validation_runs": ["run-prior"],
            }
            prior_path = root / "prior.json"
            prior_path.write_text(json.dumps(prior), encoding="utf-8")
            output = root / "LIVE_EVIDENCE.json"
            required = ["basic-exact-reply", "image-generation-history-edit"]

            with self.assertRaisesRegex(SystemExit, "without inheritance"):
                release.build_live_evidence(
                    evidence_dir, archive, output, "source-sha", "validator-sha", required
                )
            with self.assertRaisesRegex(SystemExit, "does not bind the diff base"):
                release.build_live_evidence(
                    evidence_dir,
                    archive,
                    output,
                    "source-sha",
                    "validator-sha",
                    required,
                    prior_path,
                    "other-base",
                )

            release.build_live_evidence(
                evidence_dir,
                archive,
                output,
                "source-sha",
                "validator-sha",
                required,
                prior_path,
                "base-sha",
            )
            manifest = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(manifest["required_scenarios"], required)
            self.assertEqual(set(manifest["scenarios"]), set(required))
            self.assertEqual(manifest["runner_turn_submission_count"], 3)
            inherited_identity = {
                "archive_sha256": "prior-archive-digest",
                "release_tag": "grokex-v0.149.0",
                "source_sha": "base-sha",
                "validation_run": "run-prior",
            }
            self.assertEqual(
                manifest["inherited_scenarios"],
                {
                    "encrypted-reasoning-tool-continuation": {
                        **inherited_identity,
                        "story": "grokex-encrypted-reasoning-history-continuation",
                    },
                    "ultra-full-history-collaboration": {
                        **inherited_identity,
                        "story": "grokex-provider-binding-lifecycle",
                    },
                },
            )


class ReleaseEnvelopeTest(unittest.TestCase):
    def test_packages_composes_and_verifies_release_assets(self) -> None:
        repository = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            raw_root = root / "raw"
            for target in release.TARGETS:
                suffix = ".exe" if "windows" in target else ""
                raw = raw_root / target
                raw.mkdir(parents=True)
                (raw / f"codex{suffix}").write_bytes(b"codex")
                (raw / f"codex-code-mode-host{suffix}").write_bytes(b"host")
                if "linux" in target:
                    (raw / "bwrap").write_bytes(b"bwrap")
            archives = root / "archives"
            release.package(raw_root, archives, repository, "source-sha")
            release.verify_archives(archives, "source-sha")

            archive = archives / release.archive_name("x86_64-unknown-linux-musl")
            evidence_dir = root / "evidence"
            evidence_dir.mkdir()
            for scenario, evidence in scenario_evidence(archive.name, "live-1").items():
                evidence["archive_sha256"] = release.sha256(archive)
                evidence["validator_sha"] = "source-sha"
                (evidence_dir / f"{scenario}.json").write_text(
                    json.dumps(evidence), encoding="utf-8"
                )
            live_evidence = root / "LIVE_EVIDENCE.json"
            release.build_live_evidence(
                evidence_dir, archive, live_evidence, "source-sha", "source-sha"
            )

            dist = root / "dist"
            validation_run = "candidate:1;live:live-1;release:3"
            release.build_assets(
                archives, live_evidence, dist, repository, "source-sha", validation_run
            )
            release.verify_assets(dist, "source-sha", "source-sha", validation_run, ["live-1"])

            manifest = json.loads((dist / "RELEASE.json").read_text(encoding="utf-8"))
            self.assertEqual(
                manifest,
                {
                    "archives": [release.archive_name(target) for target in release.TARGETS],
                    "source_sha": "source-sha",
                    "tag": release.TAG,
                    "upstream_commit": release.UPSTREAM_COMMIT,
                    "validation_run": validation_run,
                    "version": release.VERSION,
                },
            )
            with self.assertRaisesRegex(SystemExit, "identity mismatch"):
                release.verify_assets(dist, "source-sha", "source-sha", validation_run, ["live-2"])
            with self.assertRaisesRegex(SystemExit, "manifest mismatch"):
                release.verify_assets(dist, "source-sha", "source-sha", "other", ["live-1"])


class ObservationSummaryTest(unittest.TestCase):
    def test_summarizes_turn_latency_per_scenario(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "night-1").mkdir()
            (root / "night-2").mkdir()
            samples = [
                ("night-1", "basic-exact-reply", "completed", None, [20.0]),
                ("night-2", "basic-exact-reply", "completed", None, [40.0]),
                ("night-1", "image-generation-history-edit", None, "deadline_expired", [70.0, 180.0]),
                ("night-2", "image-generation-history-edit", "completed", None, [60.0, 90.0]),
            ]
            for night, scenario, status, outcome, durations in samples:
                evidence = {
                    "mode": "observation",
                    "scenario": scenario,
                    "turn_durations_seconds": durations,
                    **({"status": status} if status else {}),
                    **({"outcome": outcome} if outcome else {}),
                }
                (root / night / f"{scenario}.json").write_text(
                    json.dumps(evidence), encoding="utf-8"
                )
            (root / "night-1" / "release.json").write_text(
                json.dumps({"mode": "release", "scenario": "basic-exact-reply", "turn_durations_seconds": [999.0]}),
                encoding="utf-8",
            )

            self.assertEqual(
                release.summarize_observations(root),
                {
                    "basic-exact-reply": {
                        "outcomes": {"completed": 2},
                        "runs": 2,
                        "turn_deadline_seconds": 120,
                        "turn_seconds": {"max": 40.0, "over_deadline": 0, "p50": 30.0, "p95": 40.0, "samples": 2},
                    },
                    "image-generation-history-edit": {
                        "outcomes": {"completed": 1, "deadline_expired": 1},
                        "runs": 2,
                        "turn_deadline_seconds": 180,
                        "turn_seconds": {"max": 180.0, "over_deadline": 0, "p50": 80.0, "p95": 180.0, "samples": 4},
                    },
                },
            )


if __name__ == "__main__":
    unittest.main()
