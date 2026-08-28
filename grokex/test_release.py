import json
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
                "wait_path": "completed",
                "observations": {
                    "parent_response_count": 4,
                    "provider_spawn_request_count": 2,
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
                            "parent_completion": "completed",
                            "parent_result_consumption": "completed",
                            "response_assertion": "exact_match",
                            "runner_turn_submission_count": 1,
                            "semantic_acceptance": "proven",
                            "status": "completed",
                            "wait_path": "completed",
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


if __name__ == "__main__":
    unittest.main()
