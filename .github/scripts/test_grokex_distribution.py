#!/usr/bin/env python3

import pathlib
import tomllib
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]


class GrokexDistributionTest(unittest.TestCase):
    def test_example_config_keeps_chatgpt_default_and_adds_grok_profile(self) -> None:
        config_path = REPO_ROOT / "grokex" / "config.toml.example"
        config = tomllib.loads(config_path.read_text(encoding="utf-8"))

        self.assertNotIn("model", config)
        self.assertNotIn("model_provider", config)
        self.assertEqual(config["web_search"], "live")
        self.assertEqual(config["model_provider_registrations"], ["openai", "mini_grok"])
        self.assertEqual(
            config["model_providers"],
            {
                "mini_grok": {
                    "name": "Grok",
                    "base_url": "https://grok.trustedtunnel.app/v1",
                    "env_key": "GROK_API_KEY",
                    "env_key_instructions": (
                        "Set GROK_API_KEY to your Mini end-user API key."
                    ),
                    "wire_api": "grok_responses",
                    "x_search": False,
                    "requires_openai_auth": False,
                }
            },
        )

    def test_install_experience_explains_both_auth_paths(self) -> None:
        install_doc = (REPO_ROOT / "grokex" / "INSTALL.md").read_text(
            encoding="utf-8"
        )
        codex_tag = (REPO_ROOT / "grokex" / "codex-release-tag").read_text(
            encoding="utf-8"
        ).strip()
        grokex_tag = "grokex-v" + codex_tag.removeprefix("rust-v")
        unix_installer = (REPO_ROOT / "scripts" / "install-grokex.sh").read_text(
            encoding="utf-8"
        )
        windows_installer = (
            REPO_ROOT / "scripts" / "install-grokex.ps1"
        ).read_text(encoding="utf-8")

        for content in (install_doc, unix_installer, windows_installer):
            self.assertIn("grokex login", content)
            self.assertIn("GROK_API_KEY", content)

        self.assertNotIn("/releases/latest/", install_doc)
        self.assertIn(
            f"/releases/download/{grokex_tag}/install-grokex.sh",
            install_doc,
        )
        self.assertIn(
            f"/releases/download/{grokex_tag}/install-grokex.ps1",
            install_doc,
        )

    def test_rust_jobs_share_the_persistent_sccache_action(self) -> None:
        workflow = (
            REPO_ROOT / ".github" / "workflows" / "grokex-release.yml"
        ).read_text(encoding="utf-8")
        action = (
            REPO_ROOT
            / ".github"
            / "actions"
            / "setup-grokex-sccache"
            / "action.yml"
        ).read_text(encoding="utf-8")

        self.assertEqual(
            workflow.count("uses: ./.github/actions/setup-grokex-sccache"), 3
        )
        self.assertIn("uses: actions/cache@", action)
        self.assertIn("SCCACHE_GHA_ENABLED=false", action)
        self.assertIn("RUSTC_WRAPPER=${wrapper}", action)
        self.assertIn("SCCACHE_IDLE_TIMEOUT=0", action)
        self.assertIn("SCCACHE_CACHE_SIZE=10G", action)
        self.assertIn("sccache --start-server", action)
        self.assertIn("sccache --show-stats", workflow)

    def test_macos_release_matrix_matches_upstream_runner_policy(self) -> None:
        workflow = (
            REPO_ROOT / ".github" / "workflows" / "grokex-release.yml"
        ).read_text(encoding="utf-8")

        self.assertEqual(workflow.count('"runner":"macos-15"'), 2)
        self.assertEqual(workflow.count('"runner":"macos-15-intel"'), 1)
        self.assertNotIn('"runner":"macos-15-xlarge"', workflow)
        self.assertEqual(
            workflow.count(
                '"target":"aarch64-apple-darwin","archive":"tar",'
                '"timeout_minutes":130,"use_sccache":true'
            ),
            2,
        )
        self.assertEqual(
            workflow.count(
                '"target":"x86_64-apple-darwin","archive":"tar",'
                '"timeout_minutes":180,"use_sccache":false'
            ),
            1,
        )

    def test_macos_arm64_acceptance_scope_does_not_require_linux_live(self) -> None:
        workflow = (
            REPO_ROOT / ".github" / "workflows" / "grokex-release.yml"
        ).read_text(encoding="utf-8")

        self.assertIn("macos_arm64:", workflow)
        self.assertIn('build_matrix="$macos_arm64_matrix"', workflow)
        self.assertIn("includes_linux_x64=false", workflow)
        self.assertIn(
            "needs.source.outputs.includes_linux_x64 == 'true'",
            workflow,
        )
        self.assertIn(
            "Choose either full_matrix or macos_arm64, not both.",
            workflow,
        )

    def test_account_visibility_contracts_run_in_remote_ci(self) -> None:
        workflow = (
            REPO_ROOT / ".github" / "workflows" / "grokex-release.yml"
        ).read_text(encoding="utf-8")

        self.assertIn(
            "suite::v2::account::"
            "get_account_keeps_chatgpt_visible_when_current_provider_does_not_require_it",
            workflow,
        )
        self.assertIn(
            "suite::v2::account::"
            "login_account_chatgpt_redirects_to_hosted_success_page",
            workflow,
        )

    def test_rusty_v8_downloads_retry_transient_network_failures(self) -> None:
        action = (
            REPO_ROOT
            / ".github"
            / "actions"
            / "setup-rusty-v8"
            / "action.yml"
        ).read_text(encoding="utf-8")

        self.assertIn("download()", action)
        self.assertIn("--retry 5", action)
        self.assertIn("--retry-all-errors", action)
        self.assertIn('local partial="${destination}.part"', action)
        self.assertEqual(action.count('download "${base_url}/'), 3)

        musl_setup = (
            REPO_ROOT / ".github" / "scripts" / "install-musl-build-tools.sh"
        ).read_text(encoding="utf-8")
        self.assertIn("--retry 5", musl_setup)
        self.assertIn("--retry-all-errors", musl_setup)
        self.assertIn('libcap_partial="${libcap_tarball}.part"', musl_setup)

    def test_controlled_dual_provider_live_driver_is_versioned(self) -> None:
        driver = REPO_ROOT / ".github" / "scripts" / "grokex_dual_provider_live.py"
        driver_test = (
            REPO_ROOT / ".github" / "scripts" / "test_grokex_dual_provider_live.py"
        )

        self.assertTrue(driver.is_file())
        self.assertTrue(driver_test.is_file())

    def test_native_live_uses_the_shared_app_server_driver(self) -> None:
        script = (
            REPO_ROOT / ".github" / "scripts" / "grokex-live-acceptance.sh"
        ).read_text(encoding="utf-8")

        self.assertIn("grokex_dual_provider_live.py", script)
        self.assertIn("--grok-only", script)
        self.assertNotIn("grep -R", script)


if __name__ == "__main__":
    unittest.main()
