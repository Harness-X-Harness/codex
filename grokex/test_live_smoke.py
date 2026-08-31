import time
import unittest
from collections import deque

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
                "reasoning_replay": "completed",
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


if __name__ == "__main__":
    unittest.main()
