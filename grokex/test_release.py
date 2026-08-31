import json
import tempfile
import unittest
from pathlib import Path

from grokex import release


class LiveEvidenceTest(unittest.TestCase):
    def test_builds_manifest_from_both_required_scenarios(self) -> None:
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
                "operation_count": 1,
                "provider": "grok",
                "reasoning_effort": "ultra",
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
                "operation_count": 2,
                "reasoning_replay": "completed",
                "response_assertion": "exact_match",
                "scenario": "encrypted-reasoning-tool-continuation",
                "tool_continuation": "completed",
            }
            (evidence_dir / "basic.json").write_text(json.dumps(basic), encoding="utf-8")
            (evidence_dir / "continuation.json").write_text(
                json.dumps(continuation), encoding="utf-8"
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
                    "operation_count": 3,
                    "provider": "grok",
                    "reasoning_effort": "ultra",
                    "scenarios": {
                        "basic-exact-reply": {
                            "operation_count": 1,
                            "response_assertion": "exact_match",
                            "status": "completed",
                        },
                        "encrypted-reasoning-tool-continuation": {
                            "history_response_assertion": "exact_match",
                            "operation_count": 2,
                            "reasoning_replay": "completed",
                            "response_assertion": "exact_match",
                            "status": "completed",
                            "tool_continuation": "completed",
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
