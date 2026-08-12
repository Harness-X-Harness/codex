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
        unix_installer = (REPO_ROOT / "scripts" / "install-grokex.sh").read_text(
            encoding="utf-8"
        )
        windows_installer = (
            REPO_ROOT / "scripts" / "install-grokex.ps1"
        ).read_text(encoding="utf-8")

        for content in (install_doc, unix_installer, windows_installer):
            self.assertIn("grokex login", content)
            self.assertIn("GROK_API_KEY", content)

    def test_rust_jobs_share_the_github_sccache_backend(self) -> None:
        workflow = (
            REPO_ROOT / ".github" / "workflows" / "grokex-release.yml"
        ).read_text(encoding="utf-8")

        self.assertEqual(workflow.count('SCCACHE_GHA_ENABLED: "true"'), 3)
        self.assertEqual(workflow.count('RUSTC_WRAPPER: "sccache"'), 3)
        self.assertEqual(workflow.count("tool: sccache"), 3)

    def test_controlled_dual_provider_live_driver_is_versioned(self) -> None:
        driver = REPO_ROOT / ".github" / "scripts" / "grokex_dual_provider_live.py"
        driver_test = (
            REPO_ROOT / ".github" / "scripts" / "test_grokex_dual_provider_live.py"
        )

        self.assertTrue(driver.is_file())
        self.assertTrue(driver_test.is_file())


if __name__ == "__main__":
    unittest.main()
