import json
import subprocess
import tempfile
import unittest
from pathlib import Path

from grokex import release


class LiveEvidenceTest(unittest.TestCase):
    def test_builds_manifest_from_every_required_scenario(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "grokex-v0.149.0-x86_64-unknown-linux-musl.tar.gz"
            archive.write_bytes(b"candidate")
            evidence_dir = root / "evidence"
            evidence_dir.mkdir()
            common = {
                "archive": archive.name,
                "archive_sha256": "dda18a0e21ae47c53b4309434cbc02ae8bf764fa83a6defbb719431242722aa7",
                "catalog": "release-bundled",
                "model": "grok-4.6",
                "multi_agent_version": "v2",
                "provider": "grok",
                "reasoning_effort": "ultra",
                "runner_turn_submission_count": 1,
                "semantic_acceptance": "proven",
                "source_sha": "source-sha",
                "status": "completed",
                "validation_run": "run-id",
                "validator_sha": "validator-sha",
            }
            basic = {
                **common,
                "response_assertion": "exact_match",
                "scenario": "basic-exact-reply",
            }
            continuation = {
                **common,
                "history_response_assertion": "exact_match",
                "runner_turn_submission_count": 2,
                "reasoning_replay": "completed",
                "response_assertion": "exact_match",
                "scenario": "encrypted-reasoning-tool-continuation",
                "tool_continuation": "completed",
            }
            collaboration = {
                **common,
                "child_completion": "completed",
                "child_response_assertion": "exact_match",
                "default_full_history": "completed",
                "parent_completion": "completed",
                "parent_result_consumption": "completed",
                "response_assertion": "exact_match",
                "runner_turn_submission_count": 1,
                "scenario": "ultra-full-history-collaboration",
                "observations": {
                    "parent_result_consumed": True,
                    "parent_turn_status": "completed",
                    "provider_spawn_request_count": 2,
                    "provider_wait_request_count": 1,
                    "runtime_child_count": 1,
                    "runtime_spawn_completed_count": 1,
                    "runtime_spawn_failed_count": 0,
                    "target_child_reply_seen": True,
                    "target_child_turn_status": "completed",
                    "target_model_match": True,
                    "target_provider_match": True,
                    "target_runtime_child_count": 1,
                    "unpublished_canary": "must-not-enter-release-evidence",
                    "wait_completed_count": 0,
                    "wait_correlated_call_count": 0,
                    "wait_correlated_to_target": False,
                    "wait_failed_count": 0,
                    "wait_started_count": 0,
                },
            }
            (evidence_dir / "basic.json").write_text(json.dumps(basic), encoding="utf-8")
            (evidence_dir / "continuation.json").write_text(
                json.dumps(continuation), encoding="utf-8"
            )
            (evidence_dir / "collaboration.json").write_text(
                json.dumps(collaboration), encoding="utf-8"
            )
            output = root / "LIVE_EVIDENCE.json"

            release.build_live_evidence(
                evidence_dir,
                archive,
                output,
                "source-sha",
                "validator-sha",
                "run-id",
            )

            self.assertEqual(
                json.loads(output.read_text(encoding="utf-8")),
                {
                    "archive": archive.name,
                    "archive_sha256": "dda18a0e21ae47c53b4309434cbc02ae8bf764fa83a6defbb719431242722aa7",
                    "catalog": "release-bundled",
                    "model": "grok-4.6",
                    "multi_agent_version": "v2",
                    "provider": "grok",
                    "reasoning_effort": "ultra",
                    "runner_turn_submission_count": 4,
                    "scenarios": {
                        "basic-exact-reply": {
                            "response_assertion": "exact_match",
                            "runner_turn_submission_count": 1,
                            "semantic_acceptance": "proven",
                            "status": "completed",
                        },
                        "encrypted-reasoning-tool-continuation": {
                            "history_response_assertion": "exact_match",
                            "reasoning_replay": "completed",
                            "response_assertion": "exact_match",
                            "runner_turn_submission_count": 2,
                            "semantic_acceptance": "proven",
                            "status": "completed",
                            "tool_continuation": "completed",
                        },
                        "ultra-full-history-collaboration": {
                            "child_completion": "completed",
                            "child_response_assertion": "exact_match",
                            "default_full_history": "completed",
                            "observations": {
                                "parent_result_consumed": True,
                                "parent_turn_status": "completed",
                                "provider_spawn_request_count": 2,
                                "provider_wait_request_count": 1,
                                "runtime_child_count": 1,
                                "runtime_spawn_completed_count": 1,
                                "runtime_spawn_failed_count": 0,
                                "target_child_reply_seen": True,
                                "target_child_turn_status": "completed",
                                "target_model_match": True,
                                "target_provider_match": True,
                                "target_runtime_child_count": 1,
                                "wait_completed_count": 0,
                                "wait_correlated_call_count": 0,
                                "wait_correlated_to_target": False,
                                "wait_failed_count": 0,
                                "wait_started_count": 0,
                            },
                            "parent_completion": "completed",
                            "parent_result_consumption": "completed",
                            "response_assertion": "exact_match",
                            "runner_turn_submission_count": 1,
                            "semantic_acceptance": "proven",
                            "status": "completed",
                        },
                    },
                    "source_sha": "source-sha",
                    "status": "completed",
                    "validation_run": "run-id",
                    "validator_sha": "validator-sha",
                },
            )

            collaboration["semantic_acceptance"] = "not_proven"
            (evidence_dir / "collaboration.json").write_text(
                json.dumps(collaboration), encoding="utf-8"
            )
            with self.assertRaisesRegex(
                SystemExit,
                "live scenario outcome mismatch: ultra-full-history-collaboration",
            ):
                release.build_live_evidence(
                    evidence_dir,
                    archive,
                    output,
                    "source-sha",
                    "validator-sha",
                    "run-id",
                )

            collaboration["semantic_acceptance"] = "proven"
            collaboration["observations"]["target_model_match"] = False
            (evidence_dir / "collaboration.json").write_text(
                json.dumps(collaboration), encoding="utf-8"
            )
            with self.assertRaisesRegex(
                SystemExit,
                "collaboration semantic observation is invalid",
            ):
                release.build_live_evidence(
                    evidence_dir,
                    archive,
                    output,
                    "source-sha",
                    "validator-sha",
                    "run-id",
                )

    def test_release_assets_bind_validator_provenance(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            raw = root / "raw"
            for target in release.TARGETS:
                target_root = raw / target
                target_root.mkdir(parents=True)
                suffix = ".exe" if "windows" in target else ""
                for name in (f"codex{suffix}", f"codex-code-mode-host{suffix}"):
                    (target_root / name).write_bytes(b"binary")
                if "linux" in target:
                    (target_root / "bwrap").write_bytes(b"sandbox")
            archives = root / "archives"
            repository = Path(__file__).resolve().parents[1]
            release.package(raw, archives, repository, "source-sha")
            live_archive = archives / release.archive_name(
                "x86_64-unknown-linux-musl"
            )
            evidence_dir = root / "evidence"
            evidence_dir.mkdir()
            common = {
                "archive": live_archive.name,
                "archive_sha256": release.sha256(live_archive),
                "catalog": "release-bundled",
                "model": "grok-4.6",
                "multi_agent_version": "v2",
                "provider": "grok",
                "reasoning_effort": "ultra",
                "source_sha": "source-sha",
                "validation_run": "run-id",
                "validator_sha": "validator-sha",
            }
            observations = {
                "parent_result_consumed": True,
                "parent_turn_status": "completed",
                "provider_spawn_request_count": 2,
                "provider_wait_request_count": 1,
                "runtime_child_count": 1,
                "runtime_spawn_completed_count": 1,
                "runtime_spawn_failed_count": 0,
                "target_child_reply_seen": True,
                "target_child_turn_status": "completed",
                "target_model_match": True,
                "target_provider_match": True,
                "target_runtime_child_count": 1,
                "wait_completed_count": 1,
                "wait_correlated_call_count": 1,
                "wait_correlated_to_target": True,
                "wait_failed_count": 0,
                "wait_started_count": 1,
            }
            for scenario, assertions in release.LIVE_SCENARIO_ASSERTIONS.items():
                scenario_evidence = {**common, **assertions, "scenario": scenario}
                if scenario == "ultra-full-history-collaboration":
                    scenario_evidence["observations"] = observations
                (evidence_dir / f"{scenario}.json").write_text(
                    json.dumps(scenario_evidence), encoding="utf-8"
                )
            live_evidence = root / "LIVE_EVIDENCE.json"
            release.build_live_evidence(
                evidence_dir,
                live_archive,
                live_evidence,
                "source-sha",
                "validator-sha",
                "run-id",
            )
            assets = root / "assets"
            release.build_assets(
                archives,
                live_evidence,
                assets,
                repository,
                "source-sha",
                "validator-sha",
                "run-id",
            )

            release.verify_assets(
                assets, "source-sha", "validator-sha", "run-id"
            )
            self.assertEqual(
                json.loads((assets / "RELEASE.json").read_text(encoding="utf-8"))[
                    "validator_sha"
                ],
                "validator-sha",
            )
            tampered_evidence = json.loads(
                (assets / "LIVE_EVIDENCE.json").read_text(encoding="utf-8")
            )
            tampered_evidence["validator_sha"] = "wrong-validator"
            (assets / "LIVE_EVIDENCE.json").write_text(
                json.dumps(tampered_evidence), encoding="utf-8"
            )
            with self.assertRaisesRegex(SystemExit, "live evidence mismatch"):
                release.verify_assets(
                    assets, "source-sha", "validator-sha", "run-id"
                )


class ReleaseWorkflowTest(unittest.TestCase):
    def test_preflight_allowlist_matches_validator_diff(self) -> None:
        repository = Path(__file__).resolve().parents[1]
        workflow = (repository / ".github/workflows/grokex-release.yml").read_text(
            encoding="utf-8"
        )
        allowed_paths = {
            line.strip()
            for line in workflow.split(
                "          cat >\"${allowed_paths}\" <<'EOF'\n", 1
            )[1]
            .split("          EOF\n", 1)[0]
            .splitlines()
        }
        source_sha = workflow.split("  EXPECTED_SOURCE_SHA: ", 1)[1].splitlines()[0]
        observed_paths = set(
            subprocess.check_output(
                ["git", "diff", "--name-only", f"{source_sha}...HEAD"],
                cwd=repository,
                text=True,
            ).splitlines()
        )

        self.assertEqual(allowed_paths, observed_paths)

    def test_publish_claims_exact_tag_before_creating_release(self) -> None:
        repository = Path(__file__).resolve().parents[1]
        workflow = (repository / ".github/workflows/grokex-release.yml").read_text(
            encoding="utf-8"
        )
        publish_step = workflow.split("      - name: Publish once\n", 1)[1].split(
            "      - name: Download and verify published destination\n", 1
        )[0]

        self.assertIn(
            'gh api --method POST "repos/${GITHUB_REPOSITORY}/git/refs"', publish_step
        )
        self.assertIn('-f ref="refs/tags/${RELEASE_TAG}"', publish_step)
        self.assertIn(
            '-f sha="${{ needs.preflight.outputs.source_sha }}"', publish_step
        )
        self.assertIn("--verify-tag", publish_step)
        self.assertIn(
            '--target "${{ needs.preflight.outputs.source_sha }}"', publish_step
        )


if __name__ == "__main__":
    unittest.main()
