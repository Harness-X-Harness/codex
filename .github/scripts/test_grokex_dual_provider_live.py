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
    def test_wait_turn_requires_the_exact_assistant_marker(self) -> None:
        class FakeServer:
            def __init__(self, text: str) -> None:
                self.text = text

            def wait_notification(self, method, predicate, timeout=300):
                self.asserted_method = method
                params = {
                    "threadId": "thread-1",
                    "turn": {
                        "status": "completed",
                        "items": [
                            {
                                "type": "agentMessage",
                                "id": "message-1",
                                "text": self.text,
                            }
                        ],
                    },
                }
                if not predicate(params):
                    raise AssertionError("test notification did not match")
                return params

        accepted = FakeServer("EXPECTED")
        MODULE._wait_turn(accepted, "thread-1", "EXPECTED")
        self.assertEqual(accepted.asserted_method, "turn/completed")

        with self.assertRaisesRegex(
            MODULE.AcceptanceError, "turn_response_marker_missing"
        ):
            MODULE._wait_turn(FakeServer("WRONG"), "thread-1", "EXPECTED")

    def test_spawn_child_verifies_persisted_child_output(self) -> None:
        class FakeServer:
            def __init__(self, child_text: str) -> None:
                self.child_text = child_text

            def request(self, method, params, timeout=90):
                if method == "turn/start":
                    return {}
                if method == "thread/list":
                    return {
                        "data": [
                            {
                                "id": "child-1",
                                "modelProvider": "grok",
                                "status": {"type": "idle"},
                            }
                        ]
                    }
                if method == "thread/read":
                    self.read_params = params
                    return {
                        "thread": {
                            "turns": [
                                {
                                    "items": [
                                        {
                                            "type": "agentMessage",
                                            "text": self.child_text,
                                        }
                                    ]
                                }
                            ]
                        }
                    }
                raise AssertionError(f"unexpected request: {method}")

        accepted = FakeServer("CHILD_OK")
        with mock.patch.object(MODULE, "_wait_turn"):
            self.assertEqual(
                MODULE._spawn_child(
                    accepted, "parent-1", "grok", "CHILD_OK"
                ),
                "child-1",
            )
        self.assertEqual(
            accepted.read_params,
            {"threadId": "child-1", "includeTurns": True},
        )

        with mock.patch.object(MODULE, "_wait_turn"):
            with self.assertRaisesRegex(
                MODULE.AcceptanceError, "subagent_response_marker_missing"
            ):
                MODULE._spawn_child(
                    FakeServer("WRONG"), "parent-1", "grok", "CHILD_OK"
                )

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

    def test_evidence_writer_is_exclusive_and_private(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "evidence.json"
            evidence = {
                "schema": "grokex_dual_provider_live/v1",
                "started_at_unix": 1,
                "completed_at_unix": 2,
                "openai": {
                    "provider": "openai",
                    "model": "gpt-model",
                    "thread_ids": ["openai-id"],
                },
                "grok": {
                    "provider": "xai",
                    "model": "grok-model",
                    "thread_ids": ["grok-id"],
                },
            }

            MODULE._write_evidence(path, evidence)

            self.assertEqual(
                MODULE.json.loads(path.read_text(encoding="utf-8")), evidence
            )
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            with self.assertRaisesRegex(
                MODULE.AcceptanceError, "evidence_output_unavailable"
            ):
                MODULE._write_evidence(path, evidence)

    def test_provider_binding_error_requires_the_expected_code_and_message(self) -> None:
        message = {
            "error": {
                "code": -32600,
                "message": "Thread is bound to openai; start a new thread.",
            }
        }

        with self.assertRaisesRegex(
            MODULE.AcceptanceError,
            r"^rpc_error:turn/start:-32600:provider_binding$",
        ):
            MODULE.AppServer._response_result("turn/start", message)

        with self.assertRaisesRegex(
            MODULE.AcceptanceError,
            r"^rpc_error:turn/start:-32600$",
        ):
            MODULE.AppServer._response_result(
                "turn/start", {"error": {"code": -32600, "message": "other"}}
            )

    def test_hosted_story_requires_turn_and_persisted_app_projection(self) -> None:
        class FakeServer:
            def __init__(self, persisted: bool) -> None:
                self.persisted = persisted

            def request(self, method, params, timeout=90):
                if method == "turn/start":
                    return {}
                if method == "thread/read":
                    items = (
                        [{"id": "search-current", "type": "webSearch", "source": "x"}]
                        if self.persisted
                        else [{"id": "search-old", "type": "webSearch", "source": "x"}]
                    )
                    return {"thread": {"turns": [{"items": items}]}}
                raise AssertionError(f"unexpected request: {method}")

        turn = {
            "items": [
                {"id": "search-current", "type": "webSearch", "source": "x"}
            ]
        }
        with mock.patch.object(MODULE, "_wait_turn", return_value=turn):
            MODULE._run_hosted_story(
                FakeServer(True),
                "thread-1",
                "use x search",
                "webSearch",
                "x",
            )
            with self.assertRaisesRegex(
                MODULE.AcceptanceError, "hosted_item_not_persisted:webSearch"
            ):
                MODULE._run_hosted_story(
                    FakeServer(False),
                    "thread-1",
                    "use x search",
                    "webSearch",
                    "x",
                )

    def test_model_selection_requires_grok_but_allows_grok_only_mode(self) -> None:
        class FakeServer:
            def __init__(self, models):
                self.models = models

            def request(self, method, params, timeout=90):
                self.asserted_method = method
                return {"data": self.models, "nextCursor": None}

        grok_only = MODULE._models(
            FakeServer(
                [
                    {
                        "model": "grok-model",
                        "displayName": "Grok · Grok Model",
                        "isDefault": True,
                    }
                ]
            )
        )
        self.assertEqual(grok_only, {"grok": "grok-model"})

        with self.assertRaisesRegex(
            MODULE.AcceptanceError, "grok_model_catalog_incomplete"
        ):
            MODULE._models(
                FakeServer(
                    [
                        {
                            "model": "gpt-model",
                            "displayName": "ChatGPT · GPT Model",
                            "isDefault": True,
                        }
                    ]
                )
            )


if __name__ == "__main__":
    unittest.main()
