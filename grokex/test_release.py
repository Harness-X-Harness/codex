import json
import tempfile
import unittest
from pathlib import Path

from grokex import release


class LiveEvidenceTest(unittest.TestCase):
    def test_release_identity_rejects_rebound_live_evidence(self) -> None:
        evidence = {
            "archive": release.archive_name("x86_64-unknown-linux-musl"),
            "catalog": "release-bundled",
            "model": "grok-4.6",
            "provider": "grok",
            "source_sha": "source-sha",
            "status": "completed",
            "validation_run": "run-id",
            "validator_sha": "source-sha",
        }
        release.verify_live_identity(evidence, "source-sha", "run-id")

        for key in ("validator_sha", "catalog", "model"):
            with self.subTest(key=key):
                tampered = {**evidence, key: "different"}
                with self.assertRaises(SystemExit):
                    release.verify_live_identity(tampered, "source-sha", "run-id")

    def test_builds_manifest_from_every_required_scenario(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "grokex-v0.151.0-x86_64-unknown-linux-musl.tar.gz"
            archive.write_bytes(b"candidate")
            evidence_dir = root / "evidence"
            evidence_dir.mkdir()
            common = {
                "archive": archive.name,
                "archive_sha256": "dda18a0e21ae47c53b4309434cbc02ae8bf764fa83a6defbb719431242722aa7",
                "catalog": "release-bundled",
                "model": "grok-4.6",
                "provider": "grok",
                "runner_turn_submission_count": 1,
                "source_sha": "source-sha",
                "status": "completed",
                "validation_run": "run-id",
                "validator_sha": "validator-sha",
            }
            basic = {
                **common,
                "response_assertion": "nonempty_agent_message",
                "scenario": "basic-exact-reply",
                "story": "grokex-provider-profile-startup",
            }
            continuation = {
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
            }
            collaboration = {
                **common,
                "child_completion": "completed",
                "child_count": 3,
                "child_provider_binding": "grok/grok-4.6",
                "child_response_assertion": "canonical_uuid_v4",
                "default_full_history": "completed",
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
            }
            image = {
                **common,
                "edit_agent_reply_seen": True,
                "edit_artifact_extension": ".webp",
                "edit_artifact_match": True,
                "edit_completion": "completed",
                "edit_image_mime": "image/webp",
                "generation_agent_reply_seen": True,
                "generation_artifact_extension": ".png",
                "generation_artifact_match": True,
                "generation_completion": "completed",
                "generation_image_mime": "image/png",
                "history_arguments_verified": True,
                "image_items_completed": 2,
                "image_items_failed": 1,
                "runner_turn_submission_count": 2,
                "same_thread": True,
                "scenario": "image-generation-history-edit",
                "story": "grokex-image-generation-history-edit",
            }
            (evidence_dir / "basic.json").write_text(json.dumps(basic), encoding="utf-8")
            (evidence_dir / "continuation.json").write_text(
                json.dumps(continuation), encoding="utf-8"
            )
            (evidence_dir / "collaboration.json").write_text(
                json.dumps(collaboration), encoding="utf-8"
            )
            (evidence_dir / "image.json").write_text(
                json.dumps(image), encoding="utf-8"
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
                    "provider": "grok",
                    "runner_turn_submission_count": 6,
                    "scenarios": {
                        "basic-exact-reply": {
                            "response_assertion": "nonempty_agent_message",
                            "runner_turn_submission_count": 1,
                            "status": "completed",
                            "story": "grokex-provider-profile-startup",
                        },
                        "encrypted-reasoning-tool-continuation": {
                            "encrypted_reasoning_observed": True,
                            "history_response_assertion": "exact_match",
                            "response_assertion": "exact_match",
                            "runner_turn_submission_count": 2,
                            "same_thread_history": "completed",
                            "status": "completed",
                            "tool_continuation": "completed",
                            "tool_request_count": 4,
                            "story": "grokex-encrypted-reasoning-history-continuation",
                        },
                        "ultra-full-history-collaboration": {
                            "child_completion": "completed",
                            "child_count": 3,
                            "child_provider_binding": "grok/grok-4.6",
                            "child_response_assertion": "canonical_uuid_v4",
                            "default_full_history": "completed",
                            "explicit_fork_spawn_count": 1,
                            "failed_collaboration_tool_count": 2,
                            "missing_spawn_identity_count": 0,
                            "multi_agent_version": "v2",
                            "parent_completion": "completed",
                            "provider_response_count": 9,
                            "reasoning_effort": "ultra",
                            "response_assertion": "child_echo_match",
                            "runner_turn_submission_count": 1,
                            "spawn_count": 2,
                            "status": "completed",
                            "unexpected_collaboration_tool_count": 1,
                            "wait_count": 3,
                            "result_delivery": "completed",
                            "story": "grokex-provider-binding-lifecycle",
                        },
                        "image-generation-history-edit": {
                            "edit_agent_reply_seen": True,
                            "edit_artifact_extension": ".webp",
                            "edit_artifact_match": True,
                            "edit_completion": "completed",
                            "edit_image_mime": "image/webp",
                            "generation_agent_reply_seen": True,
                            "generation_artifact_extension": ".png",
                            "generation_artifact_match": True,
                            "generation_completion": "completed",
                            "generation_image_mime": "image/png",
                            "history_arguments_verified": True,
                            "image_items_completed": 2,
                            "image_items_failed": 1,
                            "runner_turn_submission_count": 2,
                            "same_thread": True,
                            "status": "completed",
                            "story": "grokex-image-generation-history-edit",
                        },
                    },
                    "source_sha": "source-sha",
                    "status": "completed",
                    "validation_run": "run-id",
                    "validator_sha": "validator-sha",
                },
            )


if __name__ == "__main__":
    unittest.main()
