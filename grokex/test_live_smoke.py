import base64
import json
import tempfile
import time
import unittest
from collections import deque
from pathlib import Path
from unittest.mock import patch

from grokex import live_smoke


class FakeAppServer:
    def __init__(self, messages: list[dict[str, object]]) -> None:
        self.messages = deque(messages)
        self.sent: list[dict[str, object]] = []

    def next_message(self, deadline: float, waiting_for: str) -> dict[str, object]:
        del deadline, waiting_for
        return self.messages.popleft()

    def send(self, message: dict[str, object]) -> None:
        self.sent.append(message)


class DeadlineAfterMessages(FakeAppServer):
    def next_message(self, deadline: float, waiting_for: str) -> dict[str, object]:
        del deadline
        if not self.messages:
            raise SystemExit(
                f"App Server response deadline expired while waiting for {waiting_for}"
            )
        return self.messages.popleft()

    def send(self, message: dict[str, object]) -> None:
        self.sent.append(message)


class FakeScenarioAppServer(FakeAppServer):
    def __init__(
        self,
        messages: list[dict[str, object]],
        model: dict[str, object] | None = None,
    ) -> None:
        super().__init__(messages)
        self.requests: list[tuple[int, str, dict[str, object]]] = []
        self.model = model or {
            "id": "grok-4.6",
            "multiAgentVersion": "v2",
            "supportedReasoningEfforts": [{"reasoningEffort": "ultra"}],
        }

    def request(
        self, request_id: int, method: str, params: dict[str, object]
    ) -> dict[str, object]:
        self.requests.append((request_id, method, params))
        if method == "initialize":
            return {}
        if method == "model/list":
            return {"data": [self.model]}
        if method == "thread/start":
            return {"modelProvider": "grok", "thread": {"id": "thread-1"}}
        if method == "turn/start":
            return {}
        raise AssertionError(f"unexpected request method: {method}")

    def close(self) -> None:
        pass


class VerifiedTurnTest(unittest.TestCase):
    def test_image_scenario_uses_same_thread_and_verifies_history_arguments(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifact = root / "generated.jpg"
            jpeg = (Path(__file__).parents[1] / "codex-rs/vendor/bubblewrap/bubblewrap.jpg").read_bytes()
            artifact.write_bytes(jpeg)
            image_item = {"method": "item/completed", "params": {"item": {"type": "imageGeneration", "status": "completed", "result": base64.b64encode(jpeg).decode(), "savedPath": str(artifact)}}}
            failed_image_item = {"method": "item/completed", "params": {"item": {"type": "imageGeneration", "status": "failed"}}}
            agent_reply = {"method": "item/completed", "params": {"item": {"type": "agentMessage", "text": "done"}}}
            turn_done = {"method": "turn/completed", "params": {"turn": {"status": "completed"}}}
            raw_edit = {"method": "rawResponseItem/completed", "params": {"item": {"type": "function_call", "name": live_smoke.IMAGE_FUNCTION_WIRE_NAME, "arguments": json.dumps({"num_last_images_to_include": 2})}}}
            server = FakeScenarioAppServer(
                [
                    failed_image_item, image_item, image_item, agent_reply, turn_done,
                    raw_edit, failed_image_item, image_item, image_item, agent_reply, turn_done,
                ],
                model={"id": "grok-4.6"},
            )
            archive = root / "candidate.tar.gz"
            archive.write_bytes(b"candidate")
            config = root / "config.toml"
            config.write_text('model = "grok-4.6"\nmodel_provider = "grok"\n[model_providers.grok]\nexperimental_bearer_token = "secret"\n', encoding="utf-8")
            evidence_path = root / "evidence.json"
            with patch.object(live_smoke, "extract_archive", return_value=root), patch.object(live_smoke, "AppServer", return_value=server):
                live_smoke.run_smoke(archive, config, evidence_path, "source", "validator", "run", live_smoke.IMAGE_SCENARIO)
            turns = [request for request in server.requests if request[1] == "turn/start"]
            self.assertEqual([request[2]["threadId"] for request in turns], ["thread-1", "thread-1"])
            thread_start = next(request for request in server.requests if request[1] == "thread/start")
            self.assertTrue(thread_start[2]["experimentalRawEvents"])
            evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
            self.assertTrue(evidence["history_arguments_verified"])
            self.assertTrue(evidence["agent_reply_seen"])
            self.assertEqual(evidence["image_items_completed"], 2)
            self.assertEqual(evidence["image_items_failed"], 1)
            self.assertNotIn("result", evidence)
            self.assertNotIn(str(artifact), json.dumps(evidence))

    def test_accepts_basic_exact_reply_without_tool(self) -> None:
        server = FakeAppServer(
            [
                {
                    "method": "item/completed",
                    "params": {
                        "item": {
                            "type": "agentMessage",
                            "text": live_smoke.BASIC_EXPECTED_AGENT_REPLY,
                        }
                    },
                },
                {
                    "method": "turn/completed",
                    "params": {"turn": {"status": "completed"}},
                },
            ]
        )

        evidence = live_smoke.wait_for_basic_turn(server, time.monotonic() + 1)

        self.assertEqual(
            evidence,
            {
                "response_assertion": "exact_match",
                "status": "completed",
            },
        )
        self.assertEqual(server.sent, [])

    def completed_turn(self, reply: str, status: str = "completed") -> FakeAppServer:
        return FakeAppServer(
            [
                {
                    "method": "item/completed",
                    "params": {"item": {"type": "reasoning"}},
                },
                {
                    "id": 41,
                    "method": "item/tool/call",
                    "params": {"tool": live_smoke.TOOL_NAME, "arguments": {}},
                },
                {
                    "method": "item/completed",
                    "params": {
                        "item": {
                            "type": "dynamicToolCall",
                            "tool": live_smoke.TOOL_NAME,
                            "status": "completed",
                            "success": True,
                            "contentItems": [
                                {
                                    "type": "inputText",
                                    "text": live_smoke.TOOL_OUTPUT_MARKER,
                                }
                            ],
                        }
                    },
                },
                {
                    "method": "item/completed",
                    "params": {"item": {"type": "agentMessage", "text": reply}},
                },
                {
                    "method": "turn/completed",
                    "params": {"turn": {"status": status}},
                },
            ]
        )

    def test_accepts_reasoning_tool_continuation_and_exact_reply(self) -> None:
        server = self.completed_turn(live_smoke.EXPECTED_AGENT_REPLY)

        evidence = live_smoke.wait_for_verified_turn(server, time.monotonic() + 1)

        self.assertEqual(
            evidence,
            {
                "response_assertion": "exact_match",
                "status": "completed",
                "tool_continuation": "completed",
                "tool_request_count": 1,
            },
        )
        self.assertEqual(
            server.sent,
            [
                {
                    "id": 41,
                    "result": {
                        "contentItems": [
                            {
                                "type": "inputText",
                                "text": live_smoke.TOOL_OUTPUT_MARKER,
                            }
                        ],
                        "success": True,
                    },
                }
            ],
        )

    def test_repeated_semantic_tool_requests_are_diagnostic(self) -> None:
        server = self.completed_turn(live_smoke.EXPECTED_AGENT_REPLY)
        messages = list(server.messages)
        messages.insert(
            3,
            {
                "id": 42,
                "method": "item/tool/call",
                "params": {"tool": live_smoke.TOOL_NAME, "arguments": {}},
            },
        )
        server = FakeAppServer(messages)

        evidence = live_smoke.wait_for_verified_turn(server, time.monotonic() + 1)

        self.assertEqual(evidence["tool_continuation"], "completed")
        self.assertEqual(evidence["tool_request_count"], 2)
        self.assertEqual([message["id"] for message in server.sent], [41, 42])

    def test_rejects_completed_turn_with_wrong_agent_reply(self) -> None:
        server = self.completed_turn("not the expected reply")

        with self.assertRaisesRegex(SystemExit, "expected semantic reply"):
            live_smoke.wait_for_verified_turn(server, time.monotonic() + 1)

    def test_reports_secret_safe_phase_when_turn_fails_after_tool(self) -> None:
        server = self.completed_turn("", status="failed")

        with self.assertRaisesRegex(
            SystemExit,
            "status=failed.*reasoning_completed=true.*tool_requests=1.*tool_completed=true",
        ):
            live_smoke.wait_for_verified_turn(server, time.monotonic() + 1)

    def test_continuation_scenario_replays_history_in_second_turn(self) -> None:
        first_turn = self.completed_turn(live_smoke.EXPECTED_AGENT_REPLY)
        server = FakeScenarioAppServer(
            [
                *first_turn.messages,
                {
                    "method": "item/completed",
                    "params": {
                        "item": {
                            "type": "agentMessage",
                            "text": "GROKEX_HISTORY_RESPONSE_OK",
                        }
                    },
                },
                {
                    "method": "turn/completed",
                    "params": {"turn": {"status": "completed"}},
                },
            ]
        )

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "candidate.tar.gz"
            archive.write_bytes(b"candidate")
            config = root / "config.toml"
            config.write_text(
                """
model = "grok-4.6"
model_provider = "grok"

[model_providers.grok]
experimental_bearer_token = "secret"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            evidence_path = root / "evidence.json"
            with (
                patch.object(live_smoke, "extract_archive", return_value=root),
                patch.object(live_smoke, "AppServer", return_value=server),
            ):
                live_smoke.run_smoke(
                    archive,
                    config,
                    evidence_path,
                    "source-sha",
                    "validator-sha",
                    "run-id",
                    live_smoke.CONTINUATION_SCENARIO,
                )

            turn_requests = [
                request for request in server.requests if request[1] == "turn/start"
            ]
            self.assertEqual([request[0] for request in turn_requests], [4, 5])
            self.assertEqual(
                [request[2]["threadId"] for request in turn_requests],
                ["thread-1", "thread-1"],
            )
            evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
            self.assertEqual(evidence["runner_turn_submission_count"], 2)
            self.assertEqual(evidence["reasoning_replay"], "completed")
            self.assertEqual(evidence["history_response_assertion"], "exact_match")

    def collaboration_messages(
        self,
        parent_completes_first: bool = False,
        extra_response: bool = False,
    ) -> list[dict[str, object]]:
        arguments: dict[str, object] = {
            "message": (
                "Reply with exactly "
                f"{live_smoke.CHILD_EXPECTED_AGENT_REPLY} and no other text."
            ),
            "task_name": "live_child",
        }
        prefix = [
            {
                "method": "rawResponseItem/completed",
                "params": {
                    "item": {
                        "arguments": json.dumps(arguments),
                        "call_id": "spawn-1",
                        "name": "projected_spawn",
                        "type": "function_call",
                    },
                    "threadId": "thread-1",
                },
            },
            {
                "method": "item/completed",
                "params": {
                    "item": {
                        "id": "spawn-1",
                        "receiverThreadIds": ["child-1"],
                        "status": "completed",
                        "tool": "spawnAgent",
                        "type": "collabAgentToolCall",
                    },
                    "threadId": "thread-1",
                },
            },
            {
                "method": "rawResponse/completed",
                "params": {"threadId": "thread-1"},
            },
        ]
        child = [
            {
                "method": "item/completed",
                "params": {
                    "item": {
                        "text": live_smoke.CHILD_EXPECTED_AGENT_REPLY,
                        "type": "agentMessage",
                    },
                    "threadId": "child-1",
                },
            },
            {
                "method": "rawResponse/completed",
                "params": {"threadId": "child-1"},
            },
            {
                "method": "turn/completed",
                "params": {
                    "threadId": "child-1",
                    "turn": {"status": "completed"},
                },
            },
        ]
        parent = [
            {
                "method": "rawResponse/completed",
                "params": {"threadId": "thread-1"},
            },
            {
                "method": "item/completed",
                "params": {
                    "item": {
                        "status": "completed",
                        "tool": "wait",
                        "type": "collabAgentToolCall",
                    },
                    "threadId": "thread-1",
                },
            },
            {
                "method": "item/completed",
                "params": {
                    "item": {
                        "text": live_smoke.PARENT_EXPECTED_AGENT_REPLY,
                        "type": "agentMessage",
                    },
                    "threadId": "thread-1",
                },
            },
            {
                "method": "rawResponse/completed",
                "params": {"threadId": "thread-1"},
            },
            {
                "method": "turn/completed",
                "params": {
                    "threadId": "thread-1",
                    "turn": {"status": "completed"},
                },
            },
        ]
        if extra_response:
            parent.insert(
                2,
                {
                    "method": "rawResponse/completed",
                    "params": {"threadId": "thread-1"},
                },
            )
        return prefix + (parent + child if parent_completes_first else child + parent)

    def test_collaboration_scenario_proves_default_full_history(self) -> None:
        server = FakeScenarioAppServer(self.collaboration_messages())

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "candidate.tar.gz"
            archive.write_bytes(b"candidate")
            config = root / "config.toml"
            config.write_text(
                """
model = "grok-4.6"
model_provider = "grok"

[model_providers.grok]
experimental_bearer_token = "secret"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            evidence_path = root / "evidence.json"
            with (
                patch.object(live_smoke, "extract_archive", return_value=root),
                patch.object(live_smoke, "AppServer", return_value=server),
            ):
                live_smoke.run_smoke(
                    archive,
                    config,
                    evidence_path,
                    "source-sha",
                    "validator-sha",
                    "run-id",
                    live_smoke.COLLABORATION_SCENARIO,
                )

            turn_requests = [
                request for request in server.requests if request[1] == "turn/start"
            ]
            self.assertEqual(len(turn_requests), 1)
            self.assertEqual(turn_requests[0][2]["effort"], "ultra")
            thread_start = next(
                request for request in server.requests if request[1] == "thread/start"
            )
            self.assertIs(thread_start[2]["experimentalRawEvents"], True)
            evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
            self.assertEqual(evidence["runner_turn_submission_count"], 1)
            self.assertEqual(evidence["provider_response_count"], 4)
            self.assertEqual(evidence["child_count"], 1)
            self.assertEqual(evidence["default_full_history"], "completed")
            self.assertEqual(evidence["spawn_count"], 1)
            self.assertEqual(evidence["wait_count"], 1)
            self.assertEqual(evidence["child_completion"], "completed")
            self.assertEqual(evidence["parent_completion"], "completed")

    def test_collaboration_scenario_accepts_parent_completion_before_child(self) -> None:
        server = FakeAppServer(
            self.collaboration_messages(parent_completes_first=True)
        )

        evidence = live_smoke.wait_for_collaboration_turn(
            server,
            time.monotonic() + 1,
            "thread-1",
        )

        self.assertEqual(evidence["provider_response_count"], 4)
        self.assertEqual(evidence["child_completion"], "completed")

    def test_collaboration_scenario_correlates_child_when_events_arrive_first(self) -> None:
        messages = self.collaboration_messages()
        spawn_completed = messages.pop(1)
        messages.insert(5, spawn_completed)
        server = FakeAppServer(messages)

        evidence = live_smoke.wait_for_collaboration_turn(
            server,
            time.monotonic() + 1,
            "thread-1",
        )

        self.assertEqual(evidence["child_completion"], "completed")
        self.assertEqual(evidence["child_count"], 1)

    def test_collaboration_scenario_correlates_spawn_completion_before_raw_call(self) -> None:
        messages = self.collaboration_messages()
        raw_spawn = messages.pop(0)
        messages.insert(2, raw_spawn)

        evidence = live_smoke.wait_for_collaboration_turn(
            FakeAppServer(messages),
            time.monotonic() + 1,
            "thread-1",
        )

        self.assertEqual(evidence["child_completion"], "completed")
        self.assertEqual(evidence["default_full_history"], "completed")

    def test_collaboration_scenario_treats_explicit_extra_spawn_as_diagnostic(self) -> None:
        explicit_arguments = {
            "fork_turns": "none",
            "message": (
                "Reply with exactly "
                f"{live_smoke.CHILD_EXPECTED_AGENT_REPLY} and no other text."
            ),
            "task_name": "extra_child",
        }
        messages = [
            {
                "method": "rawResponseItem/completed",
                "params": {
                    "item": {
                        "arguments": json.dumps(explicit_arguments),
                        "call_id": "spawn-extra",
                        "name": "projected_spawn",
                        "type": "function_call",
                    },
                    "threadId": "thread-1",
                },
            },
            {
                "method": "item/completed",
                "params": {
                    "item": {
                        "id": "spawn-extra",
                        "receiverThreadIds": ["child-extra"],
                        "status": "completed",
                        "tool": "spawnAgent",
                        "type": "collabAgentToolCall",
                    },
                    "threadId": "thread-1",
                },
            },
            *self.collaboration_messages(),
        ]

        evidence = live_smoke.wait_for_collaboration_turn(
            FakeAppServer(messages),
            time.monotonic() + 1,
            "thread-1",
        )

        self.assertEqual(evidence["explicit_fork_spawn_count"], 1)
        self.assertEqual(evidence["child_count"], 2)

    def test_collaboration_scenario_treats_fork_turns_all_as_default_full_history(
        self,
    ) -> None:
        messages = self.collaboration_messages()
        raw_spawn = messages[0]
        raw_item = raw_spawn["params"]["item"]
        arguments = json.loads(raw_item["arguments"])
        arguments["fork_turns"] = "all"
        raw_item["arguments"] = json.dumps(arguments)
        evidence = live_smoke.wait_for_collaboration_turn(
            FakeAppServer(messages),
            time.monotonic() + 1,
            "thread-1",
        )
        self.assertEqual(evidence["default_full_history"], "completed")
        self.assertEqual(evidence["explicit_fork_spawn_count"], 0)

    def test_collaboration_scenario_correlates_sub_agent_activity_child(self) -> None:
        messages = self.collaboration_messages()
        messages = [
            message
            for message in messages
            if message.get("params", {}).get("item", {}).get("type")
            != "collabAgentToolCall"
            or message.get("params", {}).get("item", {}).get("tool") != "spawnAgent"
        ]
        messages.insert(
            1,
            {
                "method": "item/completed",
                "params": {
                    "item": {
                        "agentThreadId": "child-1",
                        "kind": "started",
                        "type": "subAgentActivity",
                    },
                    "threadId": "thread-1",
                },
            },
        )
        evidence = live_smoke.wait_for_collaboration_turn(
            FakeAppServer(messages),
            time.monotonic() + 1,
            "thread-1",
        )
        self.assertEqual(evidence["default_full_history"], "completed")
        self.assertEqual(evidence["child_count"], 1)

    def test_collaboration_scenario_treats_extra_response_as_diagnostic(self) -> None:
        server = FakeAppServer(self.collaboration_messages(extra_response=True))

        evidence = live_smoke.wait_for_collaboration_turn(
            server,
            time.monotonic() + 1,
            "thread-1",
        )

        self.assertEqual(evidence["provider_response_count"], 5)

    def test_collaboration_scenario_treats_missing_wait_as_diagnostic(self) -> None:
        server = FakeAppServer([
            message
            for message in self.collaboration_messages()
            if message.get("params", {}).get("item", {}).get("tool") != "wait"
        ])
        evidence = live_smoke.wait_for_collaboration_turn(
            server,
            time.monotonic() + 1,
            "thread-1",
        )
        self.assertEqual(evidence["wait_count"], 0)

    def test_collaboration_deadline_preserves_last_stage_without_thread_ids(self) -> None:
        spawn_arguments = {
            "message": (
                "Reply with exactly "
                f"{live_smoke.CHILD_EXPECTED_AGENT_REPLY} and no other text."
            ),
            "task_name": "live_child",
        }
        server = DeadlineAfterMessages(
            [
                {
                    "method": "rawResponseItem/completed",
                    "params": {
                        "item": {
                            "arguments": json.dumps(spawn_arguments),
                            "call_id": "spawn-1",
                            "name": "projected_spawn",
                            "type": "function_call",
                        },
                        "threadId": "thread-1",
                    },
                },
                {
                    "method": "item/completed",
                    "params": {
                        "item": {
                            "id": "spawn-1",
                            "receiverThreadIds": ["child-1"],
                            "status": "completed",
                            "tool": "spawnAgent",
                            "type": "collabAgentToolCall",
                        },
                        "threadId": "thread-1",
                    },
                },
            ]
        )

        with self.assertRaises(live_smoke.LiveDeadlineExpired) as raised:
            live_smoke.wait_for_collaboration_turn(
                server,
                time.monotonic() + 1,
                "thread-1",
            )

        evidence = raised.exception.last_stage
        self.assertEqual(evidence["outcome"], "deadline_expired")
        self.assertEqual(evidence["does_not_prove"], "product_root_cause")
        self.assertEqual(evidence["last_proven_stage"], "child_created")
        self.assertEqual(evidence["spawn_count"], 1)
        self.assertEqual(evidence["default_child_count"], 1)
        self.assertEqual(evidence["child_completed_count"], 0)
        self.assertEqual(evidence["spawn_missing_task_name_count"], 0)
        self.assertEqual(evidence["spawn_unknown_argument_key_count"], 0)
        self.assertEqual(evidence["spawn_argument_keys"], ["message", "task_name"])
        dumped = json.dumps(evidence)
        self.assertNotIn("thread-1", dumped)
        self.assertNotIn("child-1", dumped)


    def test_collaboration_deadline_records_spawn_argument_keys(self) -> None:
        spawn_arguments = {
            "message": (
                "Reply with exactly "
                f"{live_smoke.CHILD_EXPECTED_AGENT_REPLY} and no other text."
            ),
            "nickname": "live_child",
        }
        server = DeadlineAfterMessages(
            [
                {
                    "method": "rawResponseItem/completed",
                    "params": {
                        "item": {
                            "arguments": json.dumps(spawn_arguments),
                            "call_id": "spawn-1",
                            "name": "spawn_agent",
                            "namespace": "collaboration",
                            "type": "function_call",
                        },
                        "threadId": "thread-1",
                    },
                },
                {
                    "method": "item/completed",
                    "params": {
                        "item": {
                            "text": live_smoke.PARENT_EXPECTED_AGENT_REPLY,
                            "type": "agentMessage",
                        },
                        "threadId": "thread-1",
                    },
                },
                {
                    "method": "turn/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turn": {"status": "completed"},
                    },
                },
            ]
        )

        with self.assertRaises(live_smoke.LiveDeadlineExpired) as raised:
            live_smoke.wait_for_collaboration_turn(
                server,
                time.monotonic() + 1,
                "thread-1",
            )

        evidence = raised.exception.last_stage
        self.assertEqual(evidence["short_spawn_agent_count"], 1)
        self.assertEqual(evidence["spawn_namespace_counts"], {"collaboration": 1})
        self.assertEqual(evidence["spawn_missing_task_name_count"], 1)
        self.assertEqual(evidence["spawn_unknown_argument_key_count"], 1)
        self.assertEqual(evidence["spawn_argument_keys"], ["message", "nickname"])
        self.assertEqual(evidence["child_count"], 0)

    def test_collaboration_run_writes_last_stage_evidence_on_deadline(self) -> None:
        class TimeoutScenarioServer(FakeScenarioAppServer):
            def next_message(self, deadline: float, waiting_for: str) -> dict[str, object]:
                del deadline
                raise SystemExit(
                    f"App Server response deadline expired while waiting for {waiting_for}"
                )

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "candidate.tar.gz"
            archive.write_bytes(b"candidate")
            config = root / "config.toml"
            config.write_text(
                'model = "grok-4.6"\nmodel_provider = "grok"\n[model_providers.grok]\nexperimental_bearer_token = "secret"\n',
                encoding="utf-8",
            )
            evidence_path = root / "evidence.json"
            server = TimeoutScenarioServer([])
            with patch.object(live_smoke, "extract_archive", return_value=root), patch.object(
                live_smoke, "AppServer", return_value=server
            ):
                with self.assertRaises(live_smoke.LiveDeadlineExpired):
                    live_smoke.run_smoke(
                        archive,
                        config,
                        evidence_path,
                        "source",
                        "validator",
                        "run",
                        live_smoke.COLLABORATION_SCENARIO,
                    )
            evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
            self.assertEqual(evidence["outcome"], "deadline_expired")
            self.assertEqual(evidence["does_not_prove"], "product_root_cause")
            self.assertEqual(evidence["last_proven_stage"], "no_events")
            self.assertEqual(evidence["runner_turn_submission_count"], 1)
            self.assertEqual(evidence["spawn_count"], 0)
            self.assertNotIn("thread-1", json.dumps(evidence))

    def test_image_deadline_preserves_last_stage_counts(self) -> None:
        server = DeadlineAfterMessages(
            [
                {
                    "method": "item/completed",
                    "params": {
                        "item": {"type": "imageGeneration", "status": "failed"},
                    },
                }
            ]
        )

        with self.assertRaises(live_smoke.LiveDeadlineExpired) as raised:
            live_smoke.wait_for_image_turn(server, time.monotonic() + 1, False)

        evidence = raised.exception.last_stage
        self.assertEqual(evidence["outcome"], "deadline_expired")
        self.assertEqual(evidence["does_not_prove"], "product_root_cause")
        self.assertEqual(evidence["last_proven_stage"], "image_failed")
        self.assertEqual(evidence["image_items_failed"], 1)
        self.assertEqual(evidence["image_items_completed"], 0)
        self.assertEqual(evidence["image_function_call_count"], 0)


if __name__ == "__main__":
    unittest.main()
