import copy
import json
import tempfile
import time
import unittest
from collections import deque
from pathlib import Path
from queue import Queue
from unittest.mock import patch

from grokex import live_smoke


class FakeAppServer:
    def __init__(self, messages: list[dict[str, object]]) -> None:
        self.messages = deque(messages)
        self.sent: list[dict[str, object]] = []

    def next_message(self, deadline: float, waiting_for: str) -> dict[str, object]:
        del deadline, waiting_for
        if not self.messages:
            raise live_smoke.AppServerDeadline("test deadline")
        return self.messages.popleft()

    def send(self, message: dict[str, object]) -> None:
        self.sent.append(message)

    def request(
        self,
        request_id: int,
        method: str,
        params: dict[str, object],
        timeout_seconds: float = 30,
    ) -> dict[str, object]:
        del request_id, timeout_seconds
        if method != "thread/read":
            raise AssertionError(f"unexpected request method: {method}")
        thread_id = params["threadId"]
        if thread_id == "thread-1":
            items = [
                {
                    "status": "completed",
                    "tool": "wait",
                    "type": "collabAgentToolCall",
                },
                {
                    "text": live_smoke.PARENT_EXPECTED_AGENT_REPLY,
                    "type": "agentMessage",
                },
            ]
        else:
            items = [
                {
                    "text": live_smoke.CHILD_EXPECTED_AGENT_REPLY,
                    "type": "agentMessage",
                }
            ]
        return {
            "thread": {
                "modelProvider": "grok",
                "status": {"type": "idle"},
                "turns": [{"items": items, "status": "completed"}],
            }
        }


class FakeScenarioAppServer(FakeAppServer):
    def __init__(self, messages: list[dict[str, object]]) -> None:
        super().__init__(messages)
        self.requests: list[tuple[int, str, dict[str, object]]] = []

    def request(
        self,
        request_id: int,
        method: str,
        params: dict[str, object],
        timeout_seconds: float = 30,
    ) -> dict[str, object]:
        del timeout_seconds
        self.requests.append((request_id, method, params))
        if method == "initialize":
            return {}
        if method == "model/list":
            return {
                "data": [
                    {
                        "id": "grok-4.6",
                        "multiAgentVersion": "v2",
                        "supportedReasoningEfforts": [
                            {"reasoningEffort": "ultra"}
                        ],
                    }
                ]
            }
        if method == "thread/start":
            return {
                "model": "grok-4.6",
                "modelProvider": "grok",
                "thread": {"id": "thread-1"},
            }
        if method == "turn/start":
            return {}
        if method == "thread/read":
            return super().request(request_id, method, params)
        raise AssertionError(f"unexpected request method: {method}")

    def close(self) -> None:
        pass


class VerifiedTurnTest(unittest.TestCase):
    def test_app_server_request_preserves_interleaved_notifications(self) -> None:
        server = live_smoke.AppServer.__new__(live_smoke.AppServer)
        server.messages = Queue()
        server.deferred_messages = deque()
        notification = {
            "method": "item/completed",
            "params": {"item": {"type": "collabAgentToolCall"}},
        }
        server.messages.put(notification)
        server.messages.put({"id": 7, "result": {"accepted": True}})

        with patch.object(server, "send"):
            response = server.request(7, "turn/start", {})

        self.assertEqual(response, {"accepted": True})
        self.assertEqual(
            server.next_message(time.monotonic() + 1, "notification"),
            notification,
        )

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
        include_fork_turns: bool = False,
        extra_response: bool = False,
    ) -> list[dict[str, object]]:
        arguments: dict[str, object] = {
            "message": (
                "Reply with exactly "
                f"{live_smoke.CHILD_EXPECTED_AGENT_REPLY} and no other text."
            ),
            "task_name": "live_child",
        }
        if include_fork_turns:
            arguments["fork_turns"] = "all"
        prefix = [
            {
                "method": "rawResponseItem/completed",
                "params": {
                    "item": {
                        "arguments": json.dumps(arguments),
                        "call_id": "spawn-call-1",
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
                        "id": "spawn-call-1",
                        "model": "grok-4.6",
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
                "params": {
                    "responseId": "root-response-1",
                    "threadId": "thread-1",
                },
            },
            {
                "method": "item/completed",
                "params": {
                    "item": {
                        "id": "spawn-call-1",
                        "agentPath": "/root/live_child",
                        "agentThreadId": "child-1",
                        "kind": "started",
                        "type": "subAgentActivity",
                    },
                    "threadId": "thread-1",
                },
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
                "params": {
                    "responseId": "child-response-1",
                    "threadId": "child-1",
                },
            },
            {
                "method": "turn/completed",
                "params": {
                    "threadId": "child-1",
                    "turn": {"status": "completed"},
                },
            },
            {
                "method": "rawResponseItem/completed",
                "params": {
                    "item": {
                        "author": "/root/live_child",
                        "content": [
                            {
                                "text": (
                                    "Message Type: FINAL_ANSWER\n"
                                    "Task name: /root\n"
                                    "Sender: /root/live_child\n"
                                    "Payload:\n"
                                    f"{live_smoke.CHILD_EXPECTED_AGENT_REPLY}"
                                ),
                                "type": "input_text",
                            }
                        ],
                        "recipient": "/root",
                        "type": "agent_message",
                    },
                    "threadId": "thread-1",
                },
            },
        ]
        parent = [
            {
                "method": "rawResponseItem/completed",
                "params": {
                    "item": {
                        "arguments": "{}",
                        "call_id": "wait-call-1",
                        "name": "wait_agent",
                        "type": "function_call",
                    },
                    "threadId": "thread-1",
                },
            },
            {
                "method": "rawResponse/completed",
                "params": {
                    "responseId": "root-response-2",
                    "threadId": "thread-1",
                },
            },
            {
                "method": "item/started",
                "params": {
                    "item": {
                        "id": "wait-call-1",
                        "receiverThreadIds": [],
                        "status": "inProgress",
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
                        "id": "wait-call-1",
                        "receiverThreadIds": [],
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
                "params": {
                    "responseId": "root-response-3",
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
        if extra_response:
            parent.insert(
                2,
                {
                    "method": "rawResponse/completed",
                    "params": {
                        "responseId": "root-response-4",
                        "threadId": "thread-1",
                    },
                },
            )
        return prefix + child + parent

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
            prompt = turn_requests[0][2]["input"][0]["text"]
            self.assertIn(
                "emit exactly one spawn_agent call and no other tool call",
                prompt,
            )
            self.assertIn("Never call spawn_agent again", prompt)
            self.assertIn("delegated child, not the parent", prompt)
            self.assertIn("ignore the inherited parent-only serial steps", prompt)
            self.assertIn("call no tool", prompt)
            self.assertIn(
                "emit exactly one wait_agent call for that child and no other tool call",
                prompt,
            )
            thread_start = next(
                request for request in server.requests if request[1] == "thread/start"
            )
            self.assertIs(thread_start[2]["experimentalRawEvents"], True)
            self.assertIs(thread_start[2]["ephemeral"], False)
            evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
            self.assertEqual(evidence["runner_turn_submission_count"], 1)
            self.assertEqual(evidence["default_full_history"], "completed")
            self.assertEqual(evidence["semantic_acceptance"], "proven")
            self.assertEqual(evidence["child_completion"], "completed")
            self.assertEqual(evidence["parent_completion"], "completed")
            self.assertEqual(
                evidence["observations"]["provider_spawn_request_count"], 1
            )
            self.assertEqual(
                evidence["observations"]["runtime_spawn_completed_count"], 1
            )
            self.assertEqual(
                evidence["observations"]["provider_wait_request_count"], 1
            )
            self.assertEqual(evidence["observations"]["wait_started_count"], 1)
            self.assertIsNotNone(evidence["observations"]["wait_started_ms"])

    def test_collaboration_scenario_rejects_explicit_fork_turns(self) -> None:
        server = FakeAppServer(self.collaboration_messages(include_fork_turns=True))

        with self.assertRaisesRegex(SystemExit, "did not use default full history"):
            live_smoke.wait_for_collaboration_turn(
                server,
                time.monotonic() + 1,
                "thread-1",
            )

    def test_collaboration_scenario_observes_distinct_provider_spawn_calls(self) -> None:
        messages = self.collaboration_messages()
        second_spawn = copy.deepcopy(messages[0])
        second_spawn["params"]["item"]["call_id"] = "spawn-call-2"
        messages.insert(1, second_spawn)
        server = FakeAppServer(messages)

        evidence = live_smoke.wait_for_collaboration_turn(
            server,
            time.monotonic() + 1,
            "thread-1",
        )

        self.assertEqual(evidence["semantic_acceptance"], "proven")
        self.assertEqual(
            evidence["observations"]["provider_spawn_request_count"], 2
        )
        self.assertEqual(evidence["observations"]["runtime_child_count"], 1)

    def test_completed_runtime_spawn_replay_is_deduplicated(self) -> None:
        messages = self.collaboration_messages()
        messages.insert(2, copy.deepcopy(messages[1]))
        server = FakeAppServer(messages)

        evidence = live_smoke.wait_for_collaboration_turn(
            server,
            time.monotonic() + 1,
            "thread-1",
        )

        self.assertEqual(evidence["semantic_acceptance"], "proven")
        self.assertEqual(
            evidence["observations"]["runtime_spawn_completed_count"], 1
        )

    def test_v2_spawn_activity_correlates_the_target_child(self) -> None:
        messages = self.collaboration_messages()
        messages = [
            message
            for message in messages
            if message.get("params", {}).get("item", {}).get("tool")
            != "spawnAgent"
        ]
        activity = next(
            message
            for message in messages
            if message.get("params", {}).get("item", {}).get("type")
            == "subAgentActivity"
        )
        activity["params"]["item"]["id"] = "spawn-call-1"
        server = FakeAppServer(messages)

        evidence = live_smoke.wait_for_collaboration_turn(
            server,
            time.monotonic() + 1,
            "thread-1",
        )

        self.assertEqual(evidence["semantic_acceptance"], "proven")
        self.assertEqual(
            evidence["observations"]["runtime_spawn_completed_count"], 1
        )
        self.assertEqual(
            evidence["observations"]["target_runtime_child_count"], 1
        )

    def test_wait_lifecycle_replay_is_deduplicated(self) -> None:
        messages = self.collaboration_messages()
        wait_request = next(
            message
            for message in messages
            if message.get("params", {}).get("item", {}).get("name")
            == "wait_agent"
        )
        wait_started = next(
            message
            for message in messages
            if message.get("method") == "item/started"
            and message.get("params", {}).get("item", {}).get("tool") == "wait"
        )
        wait_completed = next(
            message
            for message in messages
            if message.get("method") == "item/completed"
            and message.get("params", {}).get("item", {}).get("tool") == "wait"
        )
        messages[0:0] = [
            copy.deepcopy(wait_request),
            copy.deepcopy(wait_started),
            copy.deepcopy(wait_completed),
        ]

        evidence = live_smoke.wait_for_collaboration_turn(
            FakeAppServer(messages), time.monotonic() + 1, "thread-1"
        )

        self.assertEqual(evidence["semantic_acceptance"], "proven")
        self.assertEqual(
            evidence["observations"]["provider_wait_request_count"], 1
        )
        self.assertEqual(evidence["observations"]["wait_started_count"], 1)
        self.assertEqual(evidence["observations"]["wait_completed_count"], 1)

    def test_wait_lifecycle_requires_one_correlated_call_id(self) -> None:
        messages = self.collaboration_messages()
        wait_request = next(
            message
            for message in messages
            if message.get("params", {}).get("item", {}).get("name")
            == "wait_agent"
        )
        wait_request["params"]["item"]["call_id"] = "provider-wait-call"

        with self.assertRaises(live_smoke.ScenarioFailure) as raised:
            live_smoke.wait_for_collaboration_turn(
                FakeAppServer(messages), time.monotonic() + 1, "thread-1"
            )

        evidence = raised.exception.evidence
        self.assertEqual(evidence["oracle_sufficiency"], "insufficient")
        self.assertEqual(evidence["root_cause_classification"], "inconclusive")
        self.assertEqual(evidence["trajectory_gap"], "correlated_stock_wait")
        self.assertIs(evidence["observations"]["wait_correlated_to_target"], False)

    def test_wait_lifecycle_requires_runtime_started(self) -> None:
        messages = [
            message
            for message in self.collaboration_messages()
            if not (
                message.get("method") == "item/started"
                and message.get("params", {}).get("item", {}).get("tool")
                == "wait"
            )
        ]

        with self.assertRaises(live_smoke.ScenarioFailure) as raised:
            live_smoke.wait_for_collaboration_turn(
                FakeAppServer(messages), time.monotonic() + 1, "thread-1"
            )

        evidence = raised.exception.evidence
        self.assertEqual(evidence["oracle_sufficiency"], "insufficient")
        self.assertEqual(evidence["root_cause_classification"], "inconclusive")
        self.assertEqual(evidence["trajectory_gap"], "correlated_stock_wait")
        self.assertIs(evidence["observations"]["wait_correlated_to_target"], False)

    def test_provider_wait_request_without_runtime_wait_is_diagnosed(self) -> None:
        class NoWaitSnapshotServer(FakeAppServer):
            def request(
                self,
                request_id: int,
                method: str,
                params: dict[str, object],
                timeout_seconds: float = 30,
            ) -> dict[str, object]:
                response = super().request(
                    request_id, method, params, timeout_seconds
                )
                if params["threadId"] == "thread-1":
                    items = response["thread"]["turns"][0]["items"]
                    response["thread"]["turns"][0]["items"] = [
                        item for item in items if item.get("tool") != "wait"
                    ]
                return response

        messages = self.collaboration_messages()
        messages = [
            message
            for message in messages
            if message.get("params", {}).get("item", {}).get("tool")
            not in {"spawnAgent", "wait"}
        ]
        activity = next(
            message
            for message in messages
            if message.get("params", {}).get("item", {}).get("type")
            == "subAgentActivity"
        )
        activity["params"]["item"]["id"] = "spawn-call-1"
        wait_request = {
            "method": "rawResponseItem/completed",
            "params": {
                "item": {
                    "arguments": "{}",
                    "call_id": "wait-call-1",
                    "name": "wait_agent",
                    "type": "function_call",
                },
                "threadId": "thread-1",
            },
        }
        messages.insert(-2, wait_request)
        server = NoWaitSnapshotServer(messages)

        with self.assertRaises(live_smoke.ScenarioFailure) as raised:
            live_smoke.wait_for_collaboration_turn(
                server,
                time.monotonic() + 1,
                "thread-1",
            )

        evidence = raised.exception.evidence
        self.assertEqual(evidence["oracle_sufficiency"], "insufficient")
        self.assertEqual(evidence["root_cause_classification"], "inconclusive")
        self.assertEqual(evidence["trajectory_gap"], "correlated_stock_wait")
        self.assertEqual(
            evidence["observations"]["provider_wait_request_count"], 1
        )
        self.assertEqual(evidence["observations"]["wait_started_count"], 0)
        self.assertEqual(evidence["observations"]["wait_completed_count"], 0)

    def test_selects_consumed_child_when_multiple_runtime_children_complete(self) -> None:
        messages = self.collaboration_messages()
        second_request = copy.deepcopy(messages[0])
        second_request["params"]["item"]["call_id"] = "spawn-call-2"
        second_spawn = copy.deepcopy(messages[1])
        second_spawn["params"]["item"]["id"] = "spawn-call-2"
        second_spawn["params"]["item"]["receiverThreadIds"] = ["child-0"]
        second_activity = copy.deepcopy(messages[3])
        second_activity["params"]["item"]["agentPath"] = "/root/other_child"
        second_activity["params"]["item"]["agentThreadId"] = "child-0"
        child_reply = copy.deepcopy(messages[4])
        child_reply["params"]["threadId"] = "child-0"
        child_turn = copy.deepcopy(messages[6])
        child_turn["params"]["threadId"] = "child-0"
        messages[1:1] = [
            second_request,
            second_spawn,
            second_activity,
            child_reply,
            child_turn,
        ]
        server = FakeAppServer(messages)

        evidence = live_smoke.wait_for_collaboration_turn(
            server, time.monotonic() + 1, "thread-1"
        )

        self.assertEqual(evidence["semantic_acceptance"], "proven")
        self.assertEqual(evidence["observations"]["runtime_child_count"], 2)

    def test_collaboration_scenario_observes_fourth_parent_response(self) -> None:
        server = FakeAppServer(self.collaboration_messages(extra_response=True))

        evidence = live_smoke.wait_for_collaboration_turn(
            server,
            time.monotonic() + 1,
            "thread-1",
        )

        self.assertEqual(evidence["semantic_acceptance"], "proven")
        self.assertEqual(evidence["observations"]["parent_response_count"], 4)
        self.assertEqual(evidence["observations"]["target_child_response_count"], 1)

    def test_parent_prompt_marker_without_completion_envelope_is_rejected(self) -> None:
        messages = [
            message
            for message in self.collaboration_messages()
            if message.get("params", {}).get("item", {}).get("type")
            != "agent_message"
        ]
        server = FakeAppServer(messages)

        with self.assertRaisesRegex(SystemExit, "did not consume the child result"):
            live_smoke.wait_for_collaboration_turn(
                server, time.monotonic() + 1, "thread-1"
            )

    def test_collaboration_scenario_requires_correlated_completed_wait(self) -> None:
        class NoWaitSnapshotServer(FakeAppServer):
            def request(
                self,
                request_id: int,
                method: str,
                params: dict[str, object],
                timeout_seconds: float = 30,
            ) -> dict[str, object]:
                response = super().request(
                    request_id, method, params, timeout_seconds
                )
                if params["threadId"] == "thread-1":
                    items = response["thread"]["turns"][0]["items"]
                    response["thread"]["turns"][0]["items"] = [
                        item for item in items if item.get("tool") != "wait"
                    ]
                return response

        server = NoWaitSnapshotServer(
            [
                message
                for message in self.collaboration_messages()
                if message.get("params", {}).get("item", {}).get("tool") != "wait"
            ]
        )
        with self.assertRaisesRegex(SystemExit, "did not prove a correlated stock wait"):
            live_smoke.wait_for_collaboration_turn(
                server,
                time.monotonic() + 1,
                "thread-1",
            )

    def test_wait_completion_without_child_completion_is_insufficient(self) -> None:
        class IncompleteChildServer(FakeAppServer):
            def request(
                self,
                request_id: int,
                method: str,
                params: dict[str, object],
                timeout_seconds: float = 30,
            ) -> dict[str, object]:
                response = super().request(
                    request_id, method, params, timeout_seconds
                )
                if params["threadId"] == "child-1":
                    response["thread"]["status"] = {"type": "active"}
                    response["thread"]["turns"] = [
                        {"items": [], "status": "inProgress"}
                    ]
                return response

        messages = [
            message
            for message in self.collaboration_messages()
            if message.get("params", {}).get("threadId") != "child-1"
        ]
        server = IncompleteChildServer(messages)

        with self.assertRaisesRegex(SystemExit, "target child did not complete"):
            live_smoke.wait_for_collaboration_turn(
                server,
                time.monotonic() + 1,
                "thread-1",
            )

    def test_failed_runtime_spawn_is_observed_separately(self) -> None:
        messages = self.collaboration_messages()
        failed_spawn = copy.deepcopy(messages[1])
        failed_spawn["params"]["item"]["receiverThreadIds"] = []
        failed_spawn["params"]["item"]["status"] = "failed"
        messages[1:2] = [failed_spawn, copy.deepcopy(failed_spawn)]
        messages = [
            message
            for message in messages
            if message.get("params", {}).get("threadId") != "child-1"
            and message.get("params", {}).get("item", {}).get("type")
            not in {"agent_message", "subAgentActivity"}
        ]
        server = FakeAppServer(messages)

        with self.assertRaises(live_smoke.ScenarioFailure) as raised:
            live_smoke.wait_for_collaboration_turn(
                server,
                time.monotonic() + 1,
                "thread-1",
            )

        evidence = raised.exception.evidence
        self.assertEqual(evidence["oracle_sufficiency"], "sufficient")
        self.assertEqual(evidence["root_cause_classification"], "runtime_spawn")
        self.assertEqual(
            evidence["observations"]["runtime_spawn_completed_count"], 0
        )
        self.assertEqual(evidence["observations"]["runtime_spawn_failed_count"], 1)

    def test_unrelated_failed_spawn_does_not_mask_missing_target_request(self) -> None:
        messages = self.collaboration_messages()
        unrelated_failed_spawn = copy.deepcopy(messages[1])
        unrelated_failed_spawn["params"]["item"]["id"] = "unrelated-call"
        unrelated_failed_spawn["params"]["item"]["receiverThreadIds"] = []
        unrelated_failed_spawn["params"]["item"]["status"] = "failed"
        messages[0:2] = [unrelated_failed_spawn]
        messages = [
            message
            for message in messages
            if message.get("params", {}).get("threadId") != "child-1"
            and message.get("params", {}).get("item", {}).get("type")
            != "agent_message"
        ]
        server = FakeAppServer(messages)

        with self.assertRaises(live_smoke.ScenarioFailure) as raised:
            live_smoke.wait_for_collaboration_turn(
                server,
                time.monotonic() + 1,
                "thread-1",
            )

        evidence = raised.exception.evidence
        self.assertEqual(evidence["oracle_sufficiency"], "insufficient")
        self.assertEqual(evidence["root_cause_classification"], "inconclusive")
        self.assertEqual(evidence["trajectory_gap"], "default_history_runtime_child")
        self.assertEqual(evidence["observations"]["runtime_spawn_failed_count"], 1)

    def test_deadline_without_readable_snapshots_is_inconclusive(self) -> None:
        class UnreadableServer(FakeAppServer):
            def request(
                self,
                request_id: int,
                method: str,
                params: dict[str, object],
                timeout_seconds: float = 30,
            ) -> dict[str, object]:
                del request_id, method, params, timeout_seconds
                raise live_smoke.AppServerDeadline("snapshot unavailable")

        server = UnreadableServer(self.collaboration_messages()[:2])

        with self.assertRaises(live_smoke.ScenarioFailure) as raised:
            live_smoke.wait_for_collaboration_turn(
                server,
                time.monotonic() + 1,
                "thread-1",
            )

        self.assertEqual(raised.exception.evidence["oracle_sufficiency"], "insufficient")
        self.assertEqual(
            raised.exception.evidence["root_cause_classification"], "inconclusive"
        )

    def test_deadline_writes_secret_safe_thread_snapshots(self) -> None:
        class DeadlineServer(FakeScenarioAppServer):
            def next_message(
                self, deadline: float, waiting_for: str
            ) -> dict[str, object]:
                if self.messages:
                    return super().next_message(deadline, waiting_for)
                raise live_smoke.AppServerDeadline(waiting_for)

            def request(
                self,
                request_id: int,
                method: str,
                params: dict[str, object],
                timeout_seconds: float = 30,
            ) -> dict[str, object]:
                if method != "thread/read":
                    return super().request(
                        request_id, method, params, timeout_seconds
                    )
                self.requests.append((request_id, method, params))
                child = params["threadId"] == "child-1"
                return {
                    "thread": {
                        "id": "SNAPSHOT_THREAD_ID_CANARY",
                        "modelProvider": "grok",
                        "status": {"type": "idle" if child else "active"},
                        "turns": [
                            {
                                "items": [
                                    {
                                        "text": "SNAPSHOT_REPLY_BODY_CANARY",
                                        "type": "agentMessage",
                                    },
                                    {
                                        "id": "SNAPSHOT_CALL_ID_CANARY",
                                        "prompt": "SNAPSHOT_PROMPT_BODY_CANARY",
                                        "receiverThreadIds": [
                                            "SNAPSHOT_RECEIVER_ID_CANARY"
                                        ],
                                        "status": "completed",
                                        "tool": "spawnAgent",
                                        "type": "collabAgentToolCall",
                                    },
                                    {
                                        "raw": "SNAPSHOT_RAW_TRAFFIC_CANARY",
                                        "responseId": "SNAPSHOT_RESPONSE_ID_CANARY",
                                        "type": "unsupportedRawItem",
                                    },
                                ],
                                "status": "failed" if child else "inProgress",
                            }
                        ],
                    }
                }

        messages = self.collaboration_messages()[:2]
        server = DeadlineServer(messages)

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
                with self.assertRaisesRegex(SystemExit, "semantic proof is incomplete"):
                    live_smoke.run_smoke(
                        archive,
                        config,
                        evidence_path,
                        "source-sha",
                        "validator-sha",
                        "run-id",
                        live_smoke.COLLABORATION_SCENARIO,
                    )

            evidence_text = evidence_path.read_text(encoding="utf-8")
            evidence = json.loads(evidence_text)
            self.assertEqual(evidence["status"], "failed")
            self.assertEqual(evidence["semantic_acceptance"], "not_proven")
            self.assertEqual(evidence["oracle_sufficiency"], "insufficient")
            self.assertEqual(evidence["root_cause_classification"], "inconclusive")
            self.assertEqual(evidence["last_proven_stage"], "runtime_child_created")
            self.assertEqual(evidence["trajectory_gap"], "target_child_completion")
            self.assertEqual(
                evidence["observations"]["target_child_turn_status"], "failed"
            )
            self.assertEqual(
                evidence["observations"]["thread_snapshots"]["parent"][
                    "thread_status"
                ],
                "active",
            )
            snapshot_requests = [
                request for request in server.requests if request[1] == "thread/read"
            ]
            self.assertEqual(len(snapshot_requests), 2)
            self.assertTrue(
                all(request[2]["includeTurns"] is True for request in snapshot_requests)
            )
            self.assertNotIn("thread-1", evidence_text)
            self.assertNotIn("child-1", evidence_text)
            self.assertNotIn("secret", evidence_text)
            for canary in (
                "SNAPSHOT_THREAD_ID_CANARY",
                "SNAPSHOT_CALL_ID_CANARY",
                "SNAPSHOT_RECEIVER_ID_CANARY",
                "SNAPSHOT_RESPONSE_ID_CANARY",
                "SNAPSHOT_PROMPT_BODY_CANARY",
                "SNAPSHOT_REPLY_BODY_CANARY",
                "SNAPSHOT_RAW_TRAFFIC_CANARY",
            ):
                self.assertNotIn(canary, evidence_text)


if __name__ == "__main__":
    unittest.main()
