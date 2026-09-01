import base64
import json
import queue
import tempfile
import time
import unittest
from collections import deque
from pathlib import Path
from unittest.mock import patch

from grokex import live_smoke


COLLABORATION_TOKEN = "550e8400-e29b-41d4-a716-446655440000"


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
            raise live_smoke.LiveDeadlineExpired(waiting_for, {})
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
            return {
                "model": "grok-4.6",
                "modelProvider": "grok",
                "thread": {"id": "thread-1"},
            }
        if method == "thread/resume":
            return {
                "model": "grok-4.6",
                "modelProvider": "grok",
                "thread": {"id": params["threadId"]},
            }
        if method == "turn/start":
            return {"turn": {"id": f"turn-{request_id}"}}
        raise AssertionError(f"unexpected request method: {method}")

    def close(self) -> None:
        pass


class VerifiedTurnTest(unittest.TestCase):
    def test_request_preserves_server_messages_that_precede_its_response(self) -> None:
        first_notification = {
            "id": 4,
            "method": "item/tool/call",
            "params": {},
        }
        second_notification = {"method": "item/completed", "params": {}}
        server = object.__new__(live_smoke.AppServer)
        server.messages = queue.Queue()
        server.deferred_messages = deque()
        for message in [
            first_notification,
            {"id": 4, "result": {"accepted": True}},
            second_notification,
        ]:
            server.messages.put(message)

        with patch.object(server, "send") as send:
            response = server.request(4, "turn/start", {"threadId": "thread-1"})

        self.assertEqual(response, {"accepted": True})
        send.assert_called_once()
        deadline = time.monotonic() + 1
        self.assertEqual(server.next_message(deadline, "turn"), first_notification)
        self.assertEqual(server.next_message(deadline, "turn"), second_notification)

    def test_image_scenario_uses_same_thread_and_verifies_history_arguments(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            generation_artifact = root / "generated.jpg"
            edit_artifact = root / "edited.jpg"
            jpeg = (
                Path(__file__).parents[1]
                / "codex-rs/vendor/bubblewrap/bubblewrap.jpg"
            ).read_bytes()
            generation_artifact.write_bytes(jpeg)
            edit_artifact.write_bytes(jpeg)

            def item_event(
                turn_id: str, item: dict[str, object]
            ) -> dict[str, object]:
                return {
                    "method": "item/completed",
                    "params": {
                        "item": item,
                        "threadId": "thread-1",
                        "turnId": turn_id,
                    },
                }

            def raw_event(
                turn_id: str, call_id: str, arguments: dict[str, object]
            ) -> dict[str, object]:
                return {
                    "method": "rawResponseItem/completed",
                    "params": {
                        "item": {
                            "arguments": json.dumps(arguments),
                            "call_id": call_id,
                            "name": "imagegen",
                            "namespace": "image_gen",
                            "type": "function_call",
                        },
                        "threadId": "thread-1",
                        "turnId": turn_id,
                    },
                }

            def turn_done(turn_id: str) -> dict[str, object]:
                return {
                    "method": "turn/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turn": {
                            "id": turn_id,
                            "items": [{"text": "done", "type": "agentMessage"}],
                            "status": "completed",
                        },
                    },
                }

            generation_item = item_event(
                "turn-4",
                {
                    "id": "generate-1",
                    "result": base64.b64encode(jpeg).decode(),
                    "savedPath": str(generation_artifact),
                    "status": "completed",
                    "type": "imageGeneration",
                },
            )
            edit_item = item_event(
                "turn-5",
                {
                    "id": "edit-1",
                    "result": base64.b64encode(jpeg).decode(),
                    "savedPath": str(edit_artifact),
                    "status": "completed",
                    "type": "imageGeneration",
                },
            )
            generation_reply = item_event(
                "turn-4", {"text": "done", "type": "agentMessage"}
            )
            edit_reply = item_event(
                "turn-5", {"text": "done", "type": "agentMessage"}
            )
            raw_generation = raw_event(
                "turn-4", "generate-1", {"prompt": "generate"}
            )
            raw_edit = raw_event(
                "turn-5",
                "edit-1",
                {
                    "prompt": "edit",
                    "referenced_image_paths": [str(generation_artifact)],
                },
            )
            server = FakeScenarioAppServer(
                [
                    raw_generation,
                    generation_item,
                    generation_reply,
                    turn_done("turn-4"),
                    edit_item,
                    raw_edit,
                    edit_reply,
                    turn_done("turn-5"),
                ],
                model={"id": "grok-4.6"},
            )
            archive = root / "candidate.tar.gz"
            archive.write_bytes(b"candidate")
            config = root / "config.toml"
            config.write_text('model = "grok-4.6"\nmodel_provider = "grok"\n[model_providers.grok]\nexperimental_bearer_token = "secret"\n', encoding="utf-8")
            evidence_path = root / "evidence.json"
            with patch.object(
                live_smoke, "extract_archive", return_value=root
            ), patch.object(live_smoke, "AppServer", return_value=server):
                live_smoke.run_smoke(
                    archive,
                    config,
                    evidence_path,
                    "source",
                    "validator",
                    "run",
                    live_smoke.IMAGE_SCENARIO,
                )
            turns = [
                request for request in server.requests if request[1] == "turn/start"
            ]
            self.assertEqual(
                [request[2]["threadId"] for request in turns],
                ["thread-1", "thread-1"],
            )
            thread_start = next(
                request for request in server.requests if request[1] == "thread/start"
            )
            self.assertTrue(thread_start[2]["experimentalRawEvents"])
            evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
            self.assertTrue(evidence["history_arguments_verified"])
            self.assertTrue(evidence["generation_agent_reply_seen"])
            self.assertTrue(evidence["edit_agent_reply_seen"])
            self.assertEqual(evidence["generation_completion"], "completed")
            self.assertEqual(evidence["edit_completion"], "completed")
            self.assertEqual(evidence["image_items_completed"], 2)
            self.assertEqual(evidence["image_items_failed"], 0)
            self.assertNotIn("result", evidence)
            self.assertNotIn(str(generation_artifact), json.dumps(evidence))
            self.assertNotIn(str(edit_artifact), json.dumps(evidence))

    def test_invalid_correlated_image_payload_writes_safe_stage_evidence(self) -> None:
        raw_generation = {
            "method": "rawResponseItem/completed",
            "params": {
                "item": {
                    "arguments": json.dumps({"prompt": "generate"}),
                    "call_id": "generate-1",
                    "name": "imagegen",
                    "namespace": "image_gen",
                    "type": "function_call",
                },
                "threadId": "thread-1",
                "turnId": "turn-4",
            },
        }
        invalid_image = {
            "method": "item/completed",
            "params": {
                "item": {
                    "id": "generate-1",
                    "result": base64.b64encode(b"not-an-image").decode(),
                    "savedPath": "/private/not-recorded",
                    "status": "completed",
                    "type": "imageGeneration",
                },
                "threadId": "thread-1",
                "turnId": "turn-4",
            },
        }
        server = FakeScenarioAppServer([raw_generation, invalid_image])

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
            with patch.object(
                live_smoke, "extract_archive", return_value=root
            ), patch.object(live_smoke, "AppServer", return_value=server):
                with self.assertRaises(live_smoke.LiveScenarioFailed):
                    live_smoke.run_smoke(
                        archive,
                        config,
                        evidence_path,
                        "source",
                        "validator",
                        "run",
                        live_smoke.IMAGE_SCENARIO,
                    )

            evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
            self.assertEqual(evidence["outcome"], "semantic_failure")
            self.assertEqual(evidence["last_proven_stage"], "image_payload_decoded")
            self.assertIs(evidence["history_arguments_seen"], False)
            self.assertEqual(evidence["image_function_call_count"], 1)
            dumped = json.dumps(evidence)
            self.assertNotIn("generate-1", dumped)
            self.assertNotIn("not-an-image", dumped)
            self.assertNotIn("not-recorded", dumped)

    def test_accepts_basic_terminal_agent_reply_without_tool(self) -> None:
        server = FakeAppServer(
            [
                {
                    "method": "turn/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turn": {
                            "id": "turn-4",
                            "items": [
                                {
                                    "text": live_smoke.BASIC_EXPECTED_AGENT_REPLY,
                                    "type": "agentMessage",
                                }
                            ],
                            "status": "completed",
                        },
                    },
                },
            ]
        )

        evidence = live_smoke.wait_for_basic_turn(
            server, time.monotonic() + 1, "thread-1", "turn-4"
        )

        self.assertEqual(
            evidence,
            {
                "response_assertion": "nonempty_agent_message",
                "status": "completed",
            },
        )
        self.assertEqual(server.sent, [])

    def completed_turn(self, reply: str, status: str = "completed") -> FakeAppServer:
        scoped = {"threadId": "thread-1", "turnId": "turn-4"}
        return FakeAppServer(
            [
                {
                    "method": "rawResponseItem/completed",
                    "params": {
                        "item": {
                            "encrypted_content": "opaque",
                            "type": "reasoning",
                        },
                        **scoped,
                    },
                },
                {
                    "method": "item/completed",
                    "params": {"item": {"type": "reasoning"}, **scoped},
                },
                {
                    "id": 41,
                    "method": "item/tool/call",
                    "params": {
                        "arguments": {},
                        "callId": "tool-1",
                        "namespace": None,
                        "tool": live_smoke.TOOL_NAME,
                        **scoped,
                    },
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
                        },
                        **scoped,
                    },
                },
                {
                    "method": "item/completed",
                    "params": {
                        "item": {"type": "agentMessage", "text": reply},
                        **scoped,
                    },
                },
                {
                    "method": "turn/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turn": {
                            "id": "turn-4",
                            "items": [{"text": reply, "type": "agentMessage"}],
                            "status": status,
                        },
                    },
                },
            ]
        )

    def test_accepts_reasoning_tool_continuation_and_exact_reply(self) -> None:
        server = self.completed_turn(live_smoke.EXPECTED_AGENT_REPLY)

        evidence = live_smoke.wait_for_verified_turn(
            server, time.monotonic() + 1, "thread-1", "turn-4"
        )

        self.assertEqual(
            evidence,
            {
                "encrypted_reasoning_observed": True,
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
                "params": {
                    "arguments": {},
                    "callId": "tool-2",
                    "namespace": None,
                    "threadId": "thread-1",
                    "tool": live_smoke.TOOL_NAME,
                    "turnId": "turn-4",
                },
            },
        )
        server = FakeAppServer(messages)

        evidence = live_smoke.wait_for_verified_turn(
            server, time.monotonic() + 1, "thread-1", "turn-4"
        )

        self.assertEqual(evidence["tool_continuation"], "completed")
        self.assertEqual(evidence["tool_request_count"], 2)
        self.assertEqual([message["id"] for message in server.sent], [41, 42])

    def test_rejects_completed_turn_with_wrong_agent_reply(self) -> None:
        server = self.completed_turn(f" {live_smoke.EXPECTED_AGENT_REPLY}")

        with self.assertRaises(live_smoke.LiveScenarioFailed) as raised:
            live_smoke.wait_for_verified_turn(
                server, time.monotonic() + 1, "thread-1", "turn-4"
            )

        self.assertEqual(raised.exception.last_stage["outcome"], "semantic_failure")
        self.assertEqual(raised.exception.last_stage["last_proven_stage"], "turn_completed")

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
                            "text": live_smoke.HISTORY_EXPECTED_AGENT_REPLY,
                        },
                        "threadId": "thread-1",
                        "turnId": "turn-5",
                    },
                },
                {
                    "method": "turn/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turn": {
                            "id": "turn-5",
                            "items": [
                                {
                                    "text": live_smoke.HISTORY_EXPECTED_AGENT_REPLY,
                                    "type": "agentMessage",
                                }
                            ],
                            "status": "completed",
                        },
                    },
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
            self.assertTrue(evidence["encrypted_reasoning_observed"])
            self.assertEqual(evidence["same_thread_history"], "completed")
            self.assertEqual(evidence["history_response_assertion"], "exact_match")

    def test_continuation_second_turn_request_failure_records_attempt(self) -> None:
        class SecondTurnRequestDeadline(FakeScenarioAppServer):
            def request(
                self, request_id: int, method: str, params: dict[str, object]
            ) -> dict[str, object]:
                if method == "turn/start" and request_id == 5:
                    self.requests.append((request_id, method, params))
                    raise live_smoke.LiveDeadlineExpired(method, {})
                return super().request(request_id, method, params)

        first_turn = self.completed_turn(live_smoke.EXPECTED_AGENT_REPLY)
        server = SecondTurnRequestDeadline(list(first_turn.messages))
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
            with patch.object(
                live_smoke, "extract_archive", return_value=root
            ), patch.object(live_smoke, "AppServer", return_value=server):
                with self.assertRaises(live_smoke.LiveDeadlineExpired):
                    live_smoke.run_smoke(
                        archive,
                        config,
                        evidence_path,
                        "source",
                        "validator",
                        "run",
                        live_smoke.CONTINUATION_SCENARIO,
                    )

            evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
            self.assertEqual(evidence["outcome"], "deadline_expired")
            self.assertEqual(evidence["runner_turn_submission_count"], 2)
            self.assertEqual(
                evidence["last_proven_stage"],
                "history_turn_submission_attempted",
            )
            self.assertNotIn("secret", json.dumps(evidence))

    def collaboration_messages(
        self,
        parent_completes_first: bool = False,
        extra_response: bool = False,
    ) -> list[dict[str, object]]:
        arguments: dict[str, object] = {
            "message": "Return one fresh canonical UUID v4.",
            "task_name": "live_child",
        }
        prefix = [
            {
                "method": "rawResponseItem/completed",
                "params": {
                    "item": {
                        "arguments": json.dumps(arguments),
                        "call_id": "spawn-1",
                        "name": "spawn_agent",
                        "namespace": "collaboration",
                        "type": "function_call",
                    },
                    "threadId": "thread-1",
                    "turnId": "turn-4",
                },
            },
            {
                "method": "item/completed",
                "params": {
                    "item": {
                        "agentPath": "/root/live_child",
                        "agentThreadId": "child-1",
                        "id": "spawn-1",
                        "kind": "started",
                        "type": "subAgentActivity",
                    },
                    "threadId": "thread-1",
                    "turnId": "turn-4",
                },
            },
            {
                "method": "rawResponse/completed",
                "params": {"threadId": "thread-1", "turnId": "turn-4"},
            },
        ]
        child = [
            {
                "method": "turn/started",
                "params": {
                    "threadId": "child-1",
                    "turn": {"id": "child-turn-1", "status": "inProgress"},
                },
            },
            {
                "method": "item/completed",
                "params": {
                    "item": {
                        "text": COLLABORATION_TOKEN,
                        "type": "agentMessage",
                    },
                    "threadId": "child-1",
                    "turnId": "child-turn-1",
                },
            },
            {
                "method": "rawResponse/completed",
                "params": {"threadId": "child-1", "turnId": "child-turn-1"},
            },
            {
                "method": "turn/completed",
                "params": {
                    "threadId": "child-1",
                    "turn": {
                        "id": "child-turn-1",
                        "items": [
                            {"text": COLLABORATION_TOKEN, "type": "agentMessage"}
                        ],
                        "status": "completed",
                    },
                },
            },
        ]
        parent = [
            {
                "method": "rawResponse/completed",
                "params": {"threadId": "thread-1", "turnId": "turn-4"},
            },
            {
                "method": "rawResponseItem/completed",
                "params": {
                    "item": {
                        "arguments": "{}",
                        "call_id": "wait-1",
                        "name": "wait_agent",
                        "namespace": "collaboration",
                        "type": "function_call",
                    },
                    "threadId": "thread-1",
                    "turnId": "turn-4",
                },
            },
            {
                "method": "item/completed",
                "params": {
                    "item": {
                        "agentsStates": {},
                        "receiverThreadIds": [],
                        "status": "completed",
                        "tool": "wait",
                        "type": "collabAgentToolCall",
                    },
                    "threadId": "thread-1",
                    "turnId": "turn-4",
                },
            },
            {
                "method": "item/completed",
                "params": {
                    "item": {
                        "text": COLLABORATION_TOKEN,
                        "type": "agentMessage",
                    },
                    "threadId": "thread-1",
                    "turnId": "turn-4",
                },
            },
            {
                "method": "rawResponse/completed",
                "params": {"threadId": "thread-1", "turnId": "turn-4"},
            },
            {
                "method": "turn/completed",
                "params": {
                    "threadId": "thread-1",
                    "turn": {
                        "id": "turn-4",
                        "items": [
                            {"text": COLLABORATION_TOKEN, "type": "agentMessage"}
                        ],
                        "status": "completed",
                    },
                },
            },
            {
                "method": "item/completed",
                "params": {
                    "item": {
                        "agentPath": "/root/live_child",
                        "agentThreadId": "child-1",
                        "id": "subagent-completed-turn-child",
                        "kind": "completed",
                        "type": "subAgentActivity",
                    },
                    "threadId": "thread-1",
                    "turnId": "turn-4",
                },
            },
        ]
        if extra_response:
            parent.insert(
                3,
                {
                    "method": "rawResponse/completed",
                    "params": {"threadId": "thread-1", "turnId": "turn-4"},
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
            self.assertEqual(evidence["default_full_history"], "completed")
            self.assertEqual(evidence["child_completion"], "completed")
            self.assertEqual(evidence["parent_completion"], "completed")
            self.assertEqual(evidence["result_delivery"], "completed")

    def test_collaboration_semantics_do_not_depend_on_event_order(self) -> None:
        parent_first = self.collaboration_messages(parent_completes_first=True)
        child_before_spawn_completion = self.collaboration_messages()
        spawn_completed = child_before_spawn_completion.pop(1)
        child_before_spawn_completion.insert(5, spawn_completed)
        completion_before_request = self.collaboration_messages()
        raw_spawn = completion_before_request.pop(0)
        completion_before_request.insert(2, raw_spawn)
        later_child_turn = self.collaboration_messages()
        later_child_turn[7:7] = [
            {
                "method": "turn/started",
                "params": {
                    "threadId": "child-1",
                    "turn": {"id": "child-turn-2", "status": "inProgress"},
                },
            },
            {
                "method": "turn/completed",
                "params": {
                    "threadId": "child-1",
                    "turn": {
                        "id": "child-turn-2",
                        "items": [{"text": "follow-up done", "type": "agentMessage"}],
                        "status": "completed",
                    },
                },
            },
        ]

        for messages in [
            parent_first,
            child_before_spawn_completion,
            completion_before_request,
            later_child_turn,
            self.collaboration_messages(extra_response=True),
        ]:
            with self.subTest(messages=messages):
                evidence, _ = live_smoke.wait_for_collaboration_turn(
                    FakeAppServer(messages),
                    time.monotonic() + 1,
                    "thread-1",
                    "turn-4",
                )
                self.assertEqual(evidence["child_completion"], "completed")
                self.assertEqual(evidence["default_full_history"], "completed")

    def test_collaboration_scenario_treats_explicit_extra_spawn_as_diagnostic(self) -> None:
        explicit_arguments = {
            "fork_turns": "none",
            "message": "Return one fresh canonical UUID v4.",
            "task_name": "extra_child",
        }
        messages = [
            {
                "method": "rawResponseItem/completed",
                "params": {
                    "item": {
                        "arguments": json.dumps(explicit_arguments),
                        "call_id": "spawn-extra",
                        "name": "spawn_agent",
                        "namespace": "collaboration",
                        "type": "function_call",
                    },
                    "threadId": "thread-1",
                    "turnId": "turn-4",
                },
            },
            {
                "method": "item/completed",
                "params": {
                    "item": {
                        "agentPath": "/root/extra_child",
                        "agentThreadId": "child-extra",
                        "id": "spawn-extra",
                        "kind": "started",
                        "type": "subAgentActivity",
                    },
                    "threadId": "thread-1",
                    "turnId": "turn-4",
                },
            },
            *self.collaboration_messages(),
        ]

        evidence, _ = live_smoke.wait_for_collaboration_turn(
            FakeAppServer(messages),
            time.monotonic() + 1,
            "thread-1",
            "turn-4",
        )

        self.assertEqual(evidence["default_full_history"], "completed")

    def test_collaboration_scenario_treats_fork_turns_all_as_default_full_history(
        self,
    ) -> None:
        messages = self.collaboration_messages()
        raw_spawn = messages[0]
        raw_item = raw_spawn["params"]["item"]
        arguments = json.loads(raw_item["arguments"])
        arguments["fork_turns"] = "all"
        raw_item["arguments"] = json.dumps(arguments)
        evidence, _ = live_smoke.wait_for_collaboration_turn(
            FakeAppServer(messages),
            time.monotonic() + 1,
            "thread-1",
            "turn-4",
        )
        self.assertEqual(evidence["default_full_history"], "completed")
        self.assertEqual(evidence["explicit_fork_spawn_count"], 0)

    def test_collaboration_deadline_preserves_last_stage_without_thread_ids(self) -> None:
        spawn_arguments = {
            "message": "Return one fresh canonical UUID v4.",
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
                            "name": "spawn_agent",
                            "namespace": "collaboration",
                            "type": "function_call",
                        },
                        "threadId": "thread-1",
                        "turnId": "turn-4",
                    },
                },
                {
                    "method": "item/completed",
                    "params": {
                        "item": {
                            "agentPath": "/root/live_child",
                            "agentThreadId": "child-1",
                            "id": "spawn-1",
                            "kind": "started",
                            "type": "subAgentActivity",
                        },
                        "threadId": "thread-1",
                        "turnId": "turn-4",
                    },
                },
            ]
        )

        with self.assertRaises(live_smoke.LiveDeadlineExpired) as raised:
            live_smoke.wait_for_collaboration_turn(
                server,
                time.monotonic() + 1,
                "thread-1",
                "turn-4",
            )

        evidence = raised.exception.last_stage
        self.assertEqual(evidence["outcome"], "deadline_expired")
        self.assertEqual(evidence["does_not_prove"], "product_root_cause")
        self.assertEqual(evidence["last_proven_stage"], "child_created")
        dumped = json.dumps(evidence)
        self.assertNotIn("thread-1", dumped)
        self.assertNotIn("child-1", dumped)

    def test_collaboration_run_writes_last_stage_evidence_on_deadline(self) -> None:
        class TimeoutScenarioServer(FakeScenarioAppServer):
            def next_message(self, deadline: float, waiting_for: str) -> dict[str, object]:
                del deadline
                raise live_smoke.LiveDeadlineExpired(waiting_for, {})

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
            self.assertNotIn("thread-1", json.dumps(evidence))

    def test_basic_and_continuation_deadlines_write_safe_stage_evidence(self) -> None:
        class TimeoutScenarioServer(FakeScenarioAppServer):
            def next_message(self, deadline: float, waiting_for: str) -> dict[str, object]:
                del deadline
                raise live_smoke.LiveDeadlineExpired(waiting_for, {})

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "candidate.tar.gz"
            archive.write_bytes(b"candidate")
            config = root / "config.toml"
            config.write_text(
                'model = "grok-4.6"\nmodel_provider = "grok"\n[model_providers.grok]\nexperimental_bearer_token = "secret"\n',
                encoding="utf-8",
            )
            for scenario in (
                live_smoke.BASIC_SCENARIO,
                live_smoke.CONTINUATION_SCENARIO,
            ):
                with self.subTest(scenario=scenario):
                    evidence_path = root / f"{scenario}.json"
                    server = TimeoutScenarioServer([])
                    with patch.object(
                        live_smoke, "extract_archive", return_value=root
                    ), patch.object(live_smoke, "AppServer", return_value=server):
                        with self.assertRaises(live_smoke.LiveDeadlineExpired):
                            live_smoke.run_smoke(
                                archive,
                                config,
                                evidence_path,
                                "source",
                                "validator",
                                "run",
                                scenario,
                            )
                    evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
                    self.assertEqual(evidence["outcome"], "deadline_expired")
                    self.assertEqual(evidence["does_not_prove"], "product_root_cause")
                    self.assertEqual(evidence["last_proven_stage"], "no_events")
                    self.assertNotIn("secret", json.dumps(evidence))

if __name__ == "__main__":
    unittest.main()
