"""Unit tests for grok/live/summary.py."""

import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import summary

STORIES = (
    "TestGrokBasic",
    "TestGrokEncryptedReasoningContinuation",
    "TestGrokCollaboration",
    "TestGrokImageGenerationEdit",
    "TestGrokCustomApplyPatch",
)


def _event(action: str, test: str | None = None, **fields: object) -> str:
    payload: dict[str, object] = {"Action": action}
    if test is not None:
        payload["Test"] = test
    payload.update(fields)
    return json.dumps(payload)


def _output(test: str, text: str) -> str:
    return _event("output", test, Output=text)


class SummaryTest(unittest.TestCase):
    def test_pass_run(self) -> None:
        lines = [
            _event("start", Package="github.com/Harness-X-Harness/codex/grok/live"),
            _event("run", "TestProxyCommandForwardsInterruptAndWaitsForChild"),
            _event(
                "pass",
                "TestProxyCommandForwardsInterruptAndWaitsForChild",
                Elapsed=0.01,
            ),
        ]
        for index, name in enumerate(STORIES):
            lines.append(_event("run", name))
            lines.append(_output(name, f"=== RUN   {name}\n"))
            lines.append(_event("pass", name, Elapsed=1.5 + index))
        lines.append(_event("pass", Elapsed=20))
        got = summary.render_summary(summary.parse_events("\n".join(lines) + "\n"))
        want = """## Grok Live

| Story | Result | Stage | Error marker | Backend status | Duration |
| --- | --- | --- | --- | --- | --- |
| TestGrokBasic | PASS | — | — | — | 1.50s |
| TestGrokEncryptedReasoningContinuation | PASS | — | — | — | 2.50s |
| TestGrokCollaboration | PASS | — | — | — | 3.50s |
| TestGrokImageGenerationEdit | PASS | — | — | — | 4.50s |
| TestGrokCustomApplyPatch | PASS | — | — | — | 5.50s |
"""
        self.assertEqual(got, want)

    def test_not_proven_with_all_fields(self) -> None:
        lines = [
            _event("run", "TestGrokBasic"),
            _output("TestGrokBasic", "    grok_live_harness_test.go:1: NOT_PROVEN\n"),
            _output("TestGrokBasic", "        stage=turn_wait\n"),
            _output("TestGrokBasic", "        error_marker=deadline_exceeded\n"),
            _output("TestGrokBasic", "        backend_status=400\n"),
            _event("fail", "TestGrokBasic", Elapsed=12),
            _event("run", "TestGrokEncryptedReasoningContinuation"),
            _event("pass", "TestGrokEncryptedReasoningContinuation", Elapsed=2.25),
            _event("run", "TestGrokCollaboration"),
            _event("skip", "TestGrokCollaboration", Elapsed=0),
            _event("run", "TestGrokImageGenerationEdit"),
            _event("pass", "TestGrokImageGenerationEdit", Elapsed=3),
            _event("run", "TestGrokCustomApplyPatch"),
            _event("pass", "TestGrokCustomApplyPatch", Elapsed=4),
        ]
        got = summary.render_summary(summary.parse_events("\n".join(lines) + "\n"))
        want = """## Grok Live

| Story | Result | Stage | Error marker | Backend status | Duration |
| --- | --- | --- | --- | --- | --- |
| TestGrokBasic | NOT_PROVEN | turn_wait | deadline_exceeded | 400 | 12.00s |
| TestGrokEncryptedReasoningContinuation | PASS | — | — | — | 2.25s |
| TestGrokCollaboration | SKIP | — | — | — | 0.00s |
| TestGrokImageGenerationEdit | PASS | — | — | — | 3.00s |
| TestGrokCustomApplyPatch | PASS | — | — | — | 4.00s |
"""
        self.assertEqual(got, want)

    def test_not_proven_without_backend_status(self) -> None:
        lines = [
            _event("run", "TestGrokBasic"),
            _output("TestGrokBasic", "NOT_PROVEN at catalog_lists_model: missing\n"),
            _output("TestGrokBasic", "stage=catalog_lists_model\n"),
            _output("TestGrokBasic", "wire_exchanges=1\n"),
            _event("fail", "TestGrokBasic", Elapsed=0.4),
        ]
        got = summary.render_summary(summary.parse_events("\n".join(lines) + "\n"))
        want = """## Grok Live

| Story | Result | Stage | Error marker | Backend status | Duration |
| --- | --- | --- | --- | --- | --- |
| TestGrokBasic | NOT_PROVEN | catalog_lists_model | — | — | 0.40s |
"""
        self.assertEqual(got, want)

    def test_skips_truncated_and_invalid_json_lines(self) -> None:
        lines = [
            "not json",
            '{"Action":"run","Test":"TestGrokBasic"',
            "",
            _event("run", "TestGrokBasic"),
            _event("pass", "TestGrokBasic", Elapsed=1),
            '{"Action":"fail","Test":"TestGrokCustomApplyPatch","Elapsed":',
        ]
        got = summary.render_summary(summary.parse_events("\n".join(lines) + "\n"))
        want = """## Grok Live

| Story | Result | Stage | Error marker | Backend status | Duration |
| --- | --- | --- | --- | --- | --- |
| TestGrokBasic | PASS | — | — | — | 1.00s |
"""
        self.assertEqual(got, want)

    def test_main_reads_file_and_writes_stdout(self) -> None:
        payload = "\n".join(
            [
                _event("run", "TestGrokBasic"),
                _event("pass", "TestGrokBasic", Elapsed=1.25),
            ]
        )
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "go-test.json"
            path.write_text(payload + "\n", encoding="utf-8")
            dest = Path(tmp) / "summary.md"
            self.assertEqual(summary.main([str(path), str(dest)]), 0)
            self.assertEqual(
                dest.read_text(encoding="utf-8"),
                summary.render_summary(summary.parse_events(payload + "\n")),
            )

    def test_stream_tees_events_and_echoes_plain_output(self) -> None:
        raw = "\n".join(
            [
                _event("run", "TestGrokBasic"),
                _output("TestGrokBasic", "=== RUN   TestGrokBasic\n"),
                "go: build failure printed outside JSON",
                _output("TestGrokBasic", "    stage=turn_wait\n"),
                _event("fail", "TestGrokBasic", Elapsed=2.5),
            ]
        )
        sink = io.StringIO()
        with tempfile.TemporaryDirectory() as tmp:
            dest = Path(tmp) / "go-test.json"
            summary.stream(dest, io.StringIO(raw + "\n"), sink)
            self.assertEqual(dest.read_text(encoding="utf-8"), raw + "\n")
        self.assertEqual(
            sink.getvalue(),
            "=== RUN   TestGrokBasic\ngo: build failure printed outside JSON\n    stage=turn_wait\n",
        )


if __name__ == "__main__":
    unittest.main()
