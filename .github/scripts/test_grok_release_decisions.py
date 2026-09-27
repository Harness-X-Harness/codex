import unittest

import grok_release_decisions as decisions


class ReleaseProvenanceTest(unittest.TestCase):
    def test_direct_version_line_push_has_no_merged_pr_provenance(self) -> None:
        with self.assertRaisesRegex(ValueError, "no merged pull request"):
            decisions.merged_pr_number([], "grok/rust-v0.157.1")

    def test_candidate_without_successful_cargo_proof_is_rejected(self) -> None:
        jobs = {
            "jobs": [
                {
                    "name": "Cargo",
                    "status": "completed",
                    "conclusion": "failure",
                }
            ]
        }

        self.assertFalse(decisions.cargo_proof_succeeded(jobs))

    def test_proven_candidate_satisfies_each_decision(self) -> None:
        pull_requests = [
            {
                "number": 291,
                "base": {"ref": "grok/rust-v0.157.1"},
                "merged_at": "2026-09-26T14:07:43Z",
            }
        ]
        workflow_runs = {
            "workflow_runs": [
                {
                    "id": 190,
                    "head_branch": "mechanism/proof",
                    "status": "completed",
                    "conclusion": "success",
                    "created_at": "2026-09-26T12:49:21Z",
                }
            ]
        }
        jobs = {
            "jobs": [
                {
                    "name": "Cargo",
                    "status": "completed",
                    "conclusion": "success",
                }
            ]
        }

        self.assertEqual(
            decisions.merged_pr_number(pull_requests, "grok/rust-v0.157.1"), 291
        )
        self.assertEqual(
            decisions.successful_pr_run_id(workflow_runs, "mechanism/proof"), 190
        )
        self.assertTrue(decisions.cargo_proof_succeeded(jobs))


class AppServerSchemaActivationTest(unittest.TestCase):
    def test_owner_surfaces_activate_consistency_proof(self) -> None:
        for path in (
            "codex-rs/app-server-protocol/src/lib.rs",
            "codex-rs/protocol/src/protocol.rs",
            "sdk/python/src/openai_codex/generated/v2/Thread.ts",
        ):
            with self.subTest(path=path):
                self.assertTrue(
                    decisions.app_server_schema_closure_required([path])
                )

    def test_proof_infrastructure_and_docs_do_not_activate_consistency_proof(self) -> None:
        paths = [
            ".github/workflows/grok.yml",
            ".github/scripts/grok_release_decisions.py",
            "grok/docs/release.md",
        ]

        self.assertFalse(decisions.app_server_schema_closure_required(paths))


if __name__ == "__main__":
    unittest.main()
