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


class FakeScenarioAppServer(FakeAppServer):
    def __init__(self, messages: list[dict[str, object]]) -> None:
        super().__init__(messages)
        self.requests: list[tuple[int, str, dict[str, object]]] = []

    def request(
        self, request_id: int, method: str, params: dict[str, object]
    ) -> dict[str, object]:
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
            return {"modelProvider": "grok", "thread": {"id": "thread-1"}}
        if method == "turn/start":
            return {}
        raise AssertionError(f"unexpected request method: {method}")

    def close(self) -> None:
        pass


class VerifiedTurnTest(unittest.TestCase):
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
            self.assertEqual(evidence["operation_count"], 2)
            self.assertEqual(evidence["reasoning_replay"], "completed")
            self.assertEqual(evidence["history_response_assertion"], "exact_match")

    def collaboration_messages(
        self,
        include_fork_turns: bool = False,
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
        if include_fork_turns:
            arguments["fork_turns"] = "all"
        prefix = [
            {
                "method": "rawResponseItem/completed",
                "params": {
                    "item": {
                        "arguments": json.dumps(arguments),
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
            self.assertEqual(evidence["operation_count"], 4)
            self.assertEqual(evidence["default_full_history"], "completed")
            self.assertEqual(evidence["spawn_count"], 1)
            self.assertEqual(evidence["child_completion"], "completed")
            self.assertEqual(evidence["parent_completion"], "completed")

    def test_collaboration_scenario_rejects_explicit_fork_turns(self) -> None:
        server = FakeAppServer(self.collaboration_messages(include_fork_turns=True))

        with self.assertRaisesRegex(SystemExit, "did not use default full history"):
            live_smoke.wait_for_collaboration_turn(
                server,
                time.monotonic() + 1,
                "thread-1",
            )

    def test_collaboration_scenario_accepts_parent_completion_before_child(self) -> None:
        server = FakeAppServer(
            self.collaboration_messages(parent_completes_first=True)
        )

        evidence = live_smoke.wait_for_collaboration_turn(
            server,
            time.monotonic() + 1,
            "thread-1",
        )

        self.assertEqual(evidence["operation_count"], 4)
        self.assertEqual(evidence["child_completion"], "completed")

    def test_collaboration_scenario_rejects_extra_response(self) -> None:
        server = FakeAppServer(self.collaboration_messages(extra_response=True))

        with self.assertRaisesRegex(SystemExit, "used more than three responses"):
            live_smoke.wait_for_collaboration_turn(
                server,
                time.monotonic() + 1,
                "thread-1",
            )


if __name__ == "__main__":
    unittest.main()
