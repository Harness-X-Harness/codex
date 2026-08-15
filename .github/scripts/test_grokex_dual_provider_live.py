#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import io
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
    def test_start_turn_returns_the_authoritative_turn_id(self) -> None:
        class FakeServer:
            def request(self, method, params, timeout=90):
                self.call = (method, params)
                return {
                    "turn": {
                        "id": "turn-1",
                        "status": "inProgress",
                        "items": [],
                    }
                }

        server = FakeServer()
        turn_id = MODULE._start_turn(server, "thread-1", "safe prompt")

        self.assertEqual(turn_id, "turn-1")
        self.assertEqual(
            server.call,
            (
                "turn/start",
                {
                    "threadId": "thread-1",
                    "input": [
                        {
                            "type": "text",
                            "text": "safe prompt",
                            "text_elements": [],
                        }
                    ],
                },
            ),
        )

    def test_chatgpt_subscription_visibility_is_independent_of_current_provider(self) -> None:
        class FakeServer:
            def __init__(self, account, auth_status=None):
                self.account = account
                self.auth_status = auth_status or {
                    "authMethod": "chatgpt",
                    "requiresOpenaiAuth": False,
                }
                self.calls = []

            def request(self, method, params, timeout=90):
                self.calls.append((method, params))
                return self.account if method == "account/read" else self.auth_status

        server = FakeServer(
            {
                "account": {"type": "chatgpt", "planType": "pro"},
                "requiresOpenaiAuth": False,
            }
        )
        MODULE._assert_chatgpt_subscription_visible(server)
        self.assertEqual(
            server.calls,
            [
                ("account/read", {"refreshToken": False}),
                (
                    "getAuthStatus",
                    {"includeToken": False, "refreshToken": False},
                ),
            ],
        )

        with self.assertRaisesRegex(
            MODULE.AcceptanceError, "chatgpt_subscription_not_visible"
        ):
            MODULE._assert_chatgpt_subscription_visible(
                FakeServer(
                    {"account": None, "requiresOpenaiAuth": False}
                )
            )

        with self.assertRaisesRegex(
            MODULE.AcceptanceError, "chatgpt_auth_method_not_visible"
        ):
            MODULE._assert_chatgpt_subscription_visible(
                FakeServer(
                    {
                        "account": {"type": "chatgpt", "planType": "pro"},
                        "requiresOpenaiAuth": False,
                    },
                    {"authMethod": None, "requiresOpenaiAuth": False},
                )
            )

    def test_wait_turn_uses_exact_identity_and_typed_message_evidence(self) -> None:
        class FakeServer:
            def wait_notification(self, method, predicate, timeout=300):
                self.asserted_method = method
                stale = {
                    "threadId": "thread-1",
                    "turn": {
                        "id": "turn-stale",
                        "status": "completed",
                        "items": [
                            {
                                "type": "agentMessage",
                                "id": "message-stale",
                                "text": "untrusted model text",
                            }
                        ],
                    },
                }
                self.rejected_stale = not predicate(stale)
                terminal = {
                    "threadId": "thread-1",
                    "turn": {
                        "id": "turn-expected",
                        "status": "completed",
                        "items": [
                            {
                                "type": "agentMessage",
                                "id": "message-1",
                                "text": "any model response is acceptable",
                            }
                        ],
                    },
                }
                if not predicate(terminal):
                    raise AssertionError("test notification did not match")
                return terminal

        server = FakeServer()
        turn = MODULE._wait_turn(server, "thread-1", "turn-expected")

        self.assertEqual(turn["id"], "turn-expected")
        self.assertEqual(server.asserted_method, "turn/completed")
        self.assertTrue(server.rejected_stale)

    def test_wait_turn_requires_completed_local_command_evidence(self) -> None:
        class FakeServer:
            def __init__(
                self, command_status: str, command_source: str = "agent"
            ) -> None:
                self.command_status = command_status
                self.command_source = command_source

            def wait_notification(self, method, predicate, timeout=300):
                params = {
                    "threadId": "thread-1",
                    "turn": {
                        "id": "turn-1",
                        "status": "completed",
                        "items": [
                            {"type": "agentMessage", "id": "message-1", "text": "ok"},
                            {
                                "type": "commandExecution",
                                "id": "command-1",
                                "source": self.command_source,
                                "status": self.command_status,
                                "exitCode": 0,
                            },
                        ],
                    },
                }
                if not predicate(params):
                    raise AssertionError("test notification did not match")
                return params

        MODULE._wait_turn(
            FakeServer("completed"),
            "thread-1",
            "turn-1",
            required_item_types=("agentMessage", "commandExecution"),
        )

        with self.assertRaisesRegex(
            MODULE.AcceptanceError,
            "turn_item_evidence_missing:commandExecution",
        ):
            MODULE._wait_turn(
                FakeServer("inProgress"),
                "thread-1",
                "turn-1",
                required_item_types=("agentMessage", "commandExecution"),
            )

        with self.assertRaisesRegex(
            MODULE.AcceptanceError,
            "turn_item_evidence_missing:commandExecution",
        ):
            MODULE._wait_turn(
                FakeServer("completed", "userShell"),
                "thread-1",
                "turn-1",
                required_item_types=("agentMessage", "commandExecution"),
            )

    def test_grok_only_starts_in_the_explicit_local_environment(self) -> None:
        case = self

        class FakeServer:
            def __init__(self, workspace: Path) -> None:
                self.workspace = workspace
                self.turn_prompt: str | None = None

            def request(self, method, params, timeout=90):
                if method == "thread/start":
                    case.assertEqual(
                        params["environments"],
                        [
                            {
                                "environmentId": "local",
                                "cwd": str(self.workspace),
                            }
                        ],
                    )
                    return {
                        "thread": {"id": "thread-1"},
                        "modelProvider": "grok",
                    }
                if method == "turn/start":
                    self.turn_prompt = params["input"][0]["text"]
                    return {"turn": {"id": "turn-1"}}
                raise AssertionError(f"unexpected request: {method}")

            def wait_notification(self, method, predicate, timeout=300):
                params = {
                    "threadId": "thread-1",
                    "turn": {
                        "id": "turn-1",
                        "status": "completed",
                        "items": [
                            {"type": "agentMessage", "id": "message-1"},
                        ],
                    },
                }
                if not predicate(params):
                    raise AssertionError("test notification did not match")
                return params

        with tempfile.TemporaryDirectory() as temp:
            workspace = Path(temp)
            server = FakeServer(workspace)

            thread_id = MODULE._start_grok_only(
                server, "grok-4.5", "grok", workspace
            )

            self.assertEqual(thread_id, "thread-1")
            self.assertEqual(
                server.turn_prompt,
                "Reply with a brief confirmation. Do not call any tool.",
            )

    def test_spawn_child_uses_a_new_provider_bound_completed_child(self) -> None:
        class FakeServer:
            def __init__(self) -> None:
                self.list_count = 0
                self.read_params = None

            def request(self, method, params, timeout=90):
                if method == "turn/start":
                    return {
                        "turn": {
                            "id": "parent-turn-1",
                            "status": "inProgress",
                            "items": [],
                        }
                    }
                if method == "thread/list":
                    self.list_count += 1
                    children = [
                        {
                            "id": "old-child",
                            "modelProvider": "grok",
                            "status": {"type": "idle"},
                        }
                    ]
                    if self.list_count > 1:
                        children.append(
                            {
                                "id": "new-child",
                                "modelProvider": "grok",
                                "status": {"type": "idle"},
                            }
                        )
                    return {
                        "data": children
                    }
                if method == "thread/read":
                    self.read_params = params
                    return {
                        "thread": {
                            "turns": [
                                {
                                    "id": "child-turn-1",
                                    "status": "completed",
                                    "items": [
                                        {
                                            "type": "agentMessage",
                                            "text": "arbitrary child response",
                                        }
                                    ]
                                }
                            ]
                        }
                    }
                raise AssertionError(f"unexpected request: {method}")

            def wait_notification(self, method, predicate, timeout=300):
                params = {
                    "threadId": "parent-1",
                    "turn": {
                        "id": "parent-turn-1",
                        "status": "completed",
                        "items": [
                            {
                                "type": "agentMessage",
                                "id": "parent-message-1",
                                "text": "arbitrary parent response",
                            }
                        ],
                    },
                }
                if not predicate(params):
                    raise AssertionError("test notification did not match")
                return params

        accepted = FakeServer()
        self.assertEqual(
            MODULE._spawn_child(accepted, "parent-1", "grok"),
            "new-child",
        )
        self.assertEqual(
            accepted.read_params,
            {"threadId": "new-child", "includeTurns": True},
        )

    def test_compaction_waits_for_its_exact_terminal_turn(self) -> None:
        class FakeServer:
            def request(self, method, params, timeout=90):
                self.request_call = (method, params)
                return {}

            def wait_notification(self, method, predicate, timeout=300):
                if method == "item/completed":
                    stale = {
                        "threadId": "thread-1",
                        "turnId": "turn-stale",
                        "item": {"type": "agentMessage"},
                    }
                    self.assert_rejects_stale_item = not predicate(stale)
                    return {
                        "threadId": "thread-1",
                        "turnId": "turn-compact",
                        "item": {"type": "contextCompaction"},
                    }
                stale = {
                    "threadId": "thread-1",
                    "turn": {"id": "turn-stale", "status": "completed"},
                }
                self.assert_rejects_stale_turn = not predicate(stale)
                terminal = {
                    "threadId": "thread-1",
                    "turn": {"id": "turn-compact", "status": "completed"},
                }
                if not predicate(terminal):
                    raise AssertionError("test terminal turn did not match")
                return terminal

        server = FakeServer()
        MODULE._compact(server, "thread-1")
        self.assertEqual(
            server.request_call,
            ("thread/compact/start", {"threadId": "thread-1"}),
        )
        self.assertTrue(server.assert_rejects_stale_item)
        self.assertTrue(server.assert_rejects_stale_turn)

    def test_interactive_thread_list_does_not_require_spawned_children(self) -> None:
        class FakeServer:
            def request(self, method, params, timeout=90):
                self.call = (method, params)
                return {
                    "data": [
                        {"id": "root", "modelProvider": "grok"},
                        {"id": "fork", "modelProvider": "grok"},
                    ]
                }

        server = FakeServer()
        MODULE._interactive_thread_list_has_bindings(
            server, {"root": "grok", "fork": "grok"}
        )
        self.assertEqual(server.call, ("thread/list", {"limit": 100}))

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
            self.assertEqual(config["model_provider_registrations"], ["openai", "xai"])
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

    def test_hosted_probe_uses_one_native_tool_and_required_choice(self) -> None:
        request = MODULE._hosted_probe_request("grok-4.5", "x_search")
        self.assertEqual(
            request,
            {
                "model": "grok-4.5",
                "input": [
                    {"role": "user", "content": "Use X Search to find the official xAI account."}
                ],
                "tools": [{"type": "x_search"}],
                "tool_choice": "required",
                "stream": True,
                "store": False,
            },
        )

    def test_hosted_probe_requires_the_exact_completed_item_shape(self) -> None:
        self.assertTrue(
            MODULE._is_completed_hosted_item(
                {
                    "type": "response.output_item.done",
                    "item": {"type": "web_search_call", "status": "completed"},
                },
                "web_search",
            )
        )
        self.assertTrue(
            MODULE._is_completed_hosted_item(
                {
                    "type": "response.output_item.done",
                    "item": {
                        "type": "custom_tool_call",
                        "status": "completed",
                        "name": "x_semantic_search",
                    },
                },
                "x_search",
            )
        )
        self.assertTrue(
            MODULE._is_completed_hosted_item(
                {
                    "type": "response.output_item.done",
                    "item": {
                        "type": "image_generation_call",
                        "status": "completed",
                        "result": "opaque",
                    },
                },
                "image_generation",
            )
        )
        self.assertFalse(
            MODULE._is_completed_hosted_item(
                {
                    "type": "response.output_item.done",
                    "item": {
                        "type": "custom_tool_call",
                        "status": "completed",
                        "name": "unknown",
                    },
                },
                "x_search",
            )
        )

    def test_hosted_probe_parses_sse_without_retaining_output(self) -> None:
        class FakeResponse:
            status = 200

            def __init__(self) -> None:
                self.lines = iter(
                    [
                        b'data: {"type":"response.output_item.done","item":{"type":"web_search_call","status":"completed"}}\n',
                        b"\n",
                        b"data: [DONE]\n",
                        b"\n",
                    ]
                )

            def __enter__(self):
                return self

            def __exit__(self, *_args):
                return False

            def readline(self, _limit):
                return next(self.lines, b"")

        captured = None

        def open_request(request, timeout):
            nonlocal captured
            captured = (request, timeout)
            return FakeResponse()

        with mock.patch.object(MODULE.urllib.request, "urlopen", side_effect=open_request):
            MODULE._probe_hosted_tool(
                "https://example.test/v1/responses",
                "private-token",
                "codex_cli_rs/0.148.0-alpha.5 (grokex_live_acceptance)",
                "grok-4.5",
                "web_search",
            )
        self.assertIsNotNone(captured)
        request, timeout = captured
        self.assertEqual(timeout, 300)
        self.assertEqual(
            request.get_header("User-agent"),
            "codex_cli_rs/0.148.0-alpha.5 (grokex_live_acceptance)",
        )
        self.assertEqual(request.get_header("Originator"), "codex_cli_rs")

    def test_hosted_probe_retries_only_missing_terminal_items(self) -> None:
        attempts = [
            MODULE.HostedProbeTerminalItemMissing("x_search"),
            MODULE.HostedProbeTerminalItemMissing("x_search"),
            None,
        ]
        with mock.patch.object(
            MODULE,
            "_probe_hosted_tool",
            side_effect=attempts,
        ) as probe, mock.patch.object(MODULE.time, "sleep") as sleep:
            MODULE._probe_hosted_tool_with_retry(
                "https://example.test/v1/responses",
                "private-token",
                "codex_cli_rs/0.148.0-alpha.5 (grokex_live_acceptance)",
                "grok-4.5",
                "x_search",
            )

        self.assertEqual(probe.call_count, 3)
        self.assertEqual([call.args for call in sleep.call_args_list], [(1,), (2,)])

    def test_hosted_probe_fails_closed_after_bounded_retries(self) -> None:
        with mock.patch.object(
            MODULE,
            "_probe_hosted_tool",
            side_effect=MODULE.HostedProbeTerminalItemMissing("x_search"),
        ) as probe, mock.patch.object(MODULE.time, "sleep") as sleep:
            with self.assertRaisesRegex(
                MODULE.HostedProbeTerminalItemMissing,
                r"^hosted_probe_terminal_item_missing:x_search$",
            ):
                MODULE._probe_hosted_tool_with_retry(
                    "https://example.test/v1/responses",
                    "private-token",
                    "codex_cli_rs/0.148.0-alpha.5 (grokex_live_acceptance)",
                    "grok-4.5",
                    "x_search",
                )

        self.assertEqual(probe.call_count, MODULE.HOSTED_PROBE_MAX_ATTEMPTS)
        self.assertEqual([call.args for call in sleep.call_args_list], [(1,), (2,)])

    def test_hosted_probe_does_not_retry_non_terminal_errors(self) -> None:
        with mock.patch.object(
            MODULE,
            "_probe_hosted_tool",
            side_effect=MODULE.AcceptanceError("hosted_probe_http_status:x_search"),
        ) as probe, mock.patch.object(MODULE.time, "sleep") as sleep:
            with self.assertRaisesRegex(
                MODULE.AcceptanceError,
                r"^hosted_probe_http_status:x_search$",
            ):
                MODULE._probe_hosted_tool_with_retry(
                    "https://example.test/v1/responses",
                    "private-token",
                    "codex_cli_rs/0.148.0-alpha.5 (grokex_live_acceptance)",
                    "grok-4.5",
                    "x_search",
                )

        probe.assert_called_once()
        sleep.assert_not_called()

    def test_all_hosted_probes_share_the_codex_client_identity(self) -> None:
        calls = []
        with mock.patch.object(
            MODULE,
            "_codex_live_user_agent",
            return_value="codex_cli_rs/1.2.3 (grokex_live_acceptance)",
        ), mock.patch.object(
            MODULE,
            "_grok_profile",
            return_value=(
                "grok",
                {
                    "base_url": "https://example.test/v1",
                    "wire_api": "grok_responses",
                    "experimental_bearer_token": "private-token",
                },
                "experimental_bearer_token",
            ),
        ), mock.patch.object(
            MODULE,
            "_probe_hosted_tool_with_retry",
            side_effect=lambda *args: calls.append(args),
        ):
            MODULE._run_gateway_hosted_live(
                Path("config.toml"), Path("codex"), "grok-4.5"
            )

        self.assertEqual(
            calls,
            [
                (
                    "https://example.test/v1/responses",
                    "private-token",
                    "codex_cli_rs/1.2.3 (grokex_live_acceptance)",
                    "grok-4.5",
                    tool_type,
                )
                for tool_type in ("web_search", "x_search", "image_generation")
            ],
        )

    def test_hosted_probe_classifies_cloudflare_client_rejection(self) -> None:
        error = MODULE.urllib.error.HTTPError(
            "https://example.test/v1/responses",
            403,
            "Forbidden",
            {},
            io.BytesIO(
                b'{"error_code":"1010","error_name":"browser_signature_banned"}'
            ),
        )
        try:
            self.assertEqual(
                MODULE._hosted_probe_error_classification(error),
                "1010",
            )
        finally:
            error.close()

    def test_model_selection_uses_verified_grok_slug_with_custom_provider_name(self) -> None:
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
                        "model": "grok-4.5",
                        "displayName": "Mini Grok Surface · Grok 4.5",
                        "isDefault": True,
                    }
                ]
            )
        )
        self.assertEqual(grok_only, {"grok": "grok-4.5"})

        with self.assertRaisesRegex(
            MODULE.AcceptanceError, "grok_model_catalog_incomplete"
        ):
            MODULE._models(
                FakeServer(
                    [
                        {
                            "model": "grok-unverified",
                            "displayName": "Grok · Unverified",
                            "isDefault": True,
                        }
                    ]
                )
            )

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
