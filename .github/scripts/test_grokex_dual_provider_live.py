#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import os
import stat
import tempfile
import textwrap
import unittest
from pathlib import Path
from unittest import mock


MODULE_PATH = Path(__file__).with_name("grokex_dual_provider_live.py")
SPEC = importlib.util.spec_from_file_location("grokex_dual_provider_live", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class GrokexDualProviderLiveTest(unittest.TestCase):
    def test_app_server_driver_preserves_multiple_frames_from_one_read(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            fake_server = root / "fake-codex"
            fake_server.write_text(
                textwrap.dedent(
                    """\
                    #!/usr/bin/env python3
                    import json
                    import os
                    import sys

                    first = json.loads(sys.stdin.buffer.readline())
                    frames = [
                        {"jsonrpc": "2.0", "id": first["id"], "result": {}},
                        {
                            "jsonrpc": "2.0",
                            "method": "test/ready",
                            "params": {"ready": True},
                        },
                    ]
                    os.write(
                        sys.stdout.fileno(),
                        ("\\n".join(json.dumps(frame) for frame in frames) + "\\n").encode(),
                    )
                    sys.stdin.buffer.readline()
                    second = json.loads(sys.stdin.buffer.readline())
                    os.write(
                        sys.stdout.fileno(),
                        (json.dumps({"jsonrpc": "2.0", "id": second["id"], "result": {}}) + "\\n").encode(),
                    )
                    """
                ),
                encoding="utf-8",
            )
            fake_server.chmod(0o700)
            home = root / "home"
            workspace = root / "workspace"
            home.mkdir()
            workspace.mkdir()

            server = MODULE.AppServer(fake_server, home, workspace)
            try:
                MODULE._initialize(server)
                ready = server.wait_notification(
                    "test/ready", lambda params: params.get("ready") is True, timeout=1
                )
                self.assertEqual(ready, {"ready": True})
                self.assertEqual(server.request("test/request", {}, timeout=1), {})
            finally:
                server.close()

    def test_isolated_config_keeps_only_the_grok_profile_contract(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / "source.toml"
            target = root / "target.toml"
            source.write_text(
                """
model = "grok-model"
model_provider = "xai"
model_catalog_json = "/private/catalog.json"

[model_providers.xai]
name = "Private label"
base_url = "https://private.example/v1"
experimental_bearer_token = "private-token"
wire_api = "grok_responses"
x_search = true
requires_openai_auth = false
""".strip()
                + "\n",
                encoding="utf-8",
            )

            provider = MODULE._write_isolated_config(source, target)
            config = MODULE.tomllib.loads(target.read_text(encoding="utf-8"))

            self.assertEqual(provider, "xai")
            self.assertNotIn("model", config)
            self.assertNotIn("model_provider", config)
            self.assertNotIn("model_catalog_json", config)
            self.assertTrue(config["features"]["multi_agent_v2"])
            self.assertEqual(set(config["model_providers"]), {"xai"})
            self.assertEqual(
                config["model_providers"]["xai"]["experimental_bearer_token"],
                "private-token",
            )
            self.assertEqual(stat.S_IMODE(target.stat().st_mode), 0o600)

    def test_env_key_profile_requires_the_credential_environment(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / "source.toml"
            target = root / "target.toml"
            source.write_text(
                """
[model_providers.xai]
base_url = "https://private.example/v1"
env_key = "GROKEX_TEST_KEY"
wire_api = "grok_responses"
requires_openai_auth = false
""".strip()
                + "\n",
                encoding="utf-8",
            )

            with mock.patch.dict(os.environ, {}, clear=True):
                with self.assertRaisesRegex(
                    MODULE.AcceptanceError,
                    "grok_credential_environment_is_missing",
                ):
                    MODULE._write_isolated_config(source, target)


if __name__ == "__main__":
    unittest.main()
