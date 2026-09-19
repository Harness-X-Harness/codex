#!/usr/bin/env python3
"""Render a Markdown job summary from a `go test -json` event stream.

`summary.py stream FILE` tees stdin (the `go test -json` stream) into FILE and
echoes each event's plain `Output` so the job log still reads like `go test -v`.
`summary.py SOURCE [DEST]` renders the Markdown table from a saved stream.
"""

import json
import sys
from pathlib import Path
from typing import Any, TextIO

STORY_PREFIX = "TestGrok"
RESULT_BY_ACTION = {"pass": "PASS", "fail": "NOT_PROVEN", "skip": "SKIP"}
FIELD_KEYS = ("stage", "error_marker", "backend_status")
MISSING = "—"


class StoryResult:
    def __init__(self, name: str) -> None:
        self.name = name
        self.result = ""
        self.output = ""
        self.elapsed: float | None = None


def parse_events(raw: str) -> list[dict[str, Any]]:
    events: list[dict[str, Any]] = []
    for line in raw.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        try:
            event = json.loads(stripped)
        except json.JSONDecodeError:
            continue
        if isinstance(event, dict):
            events.append(event)
    return events


def is_story_test(name: str) -> bool:
    return name.startswith(STORY_PREFIX) and "/" not in name


def collect_stories(events: list[dict[str, Any]]) -> list[StoryResult]:
    stories: dict[str, StoryResult] = {}
    order: list[str] = []
    for event in events:
        name = event.get("Test")
        if not isinstance(name, str) or not is_story_test(name):
            continue
        story = stories.get(name)
        if story is None:
            story = StoryResult(name)
            stories[name] = story
            order.append(name)
        action = event.get("Action")
        if action == "output":
            output = event.get("Output")
            if isinstance(output, str):
                story.output += output
            continue
        result = RESULT_BY_ACTION.get(str(action) if action is not None else "")
        if result is None:
            continue
        story.result = result
        elapsed = event.get("Elapsed")
        if isinstance(elapsed, (int, float)):
            story.elapsed = float(elapsed)
    return [stories[name] for name in order]


def parse_fields(output: str) -> dict[str, str]:
    found: dict[str, str] = {}
    for line in output.splitlines():
        stripped = line.strip()
        for key in FIELD_KEYS:
            prefix = f"{key}="
            if stripped.startswith(prefix):
                found[key] = stripped[len(prefix) :]
    return found


def cell(value: str) -> str:
    value = value.replace("\r\n", "\n").replace("\r", "\n").replace("\n", " ")
    value = value.replace("|", "\\|").strip()
    return value if value else MISSING


def duration_cell(elapsed: float | None) -> str:
    if elapsed is None:
        return MISSING
    return f"{elapsed:.2f}s"


def render_summary(events: list[dict[str, Any]]) -> str:
    lines = [
        "## Grok Live",
        "",
        "| Story | Result | Stage | Error marker | Backend status | Duration |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for story in collect_stories(events):
        fields = parse_fields(story.output)
        lines.append(
            "| {name} | {result} | {stage} | {marker} | {status} | {duration} |".format(
                name=cell(story.name),
                result=cell(story.result),
                stage=cell(fields.get("stage", "")),
                marker=cell(fields.get("error_marker", "")),
                status=cell(fields.get("backend_status", "")),
                duration=duration_cell(story.elapsed),
            )
        )
    return "\n".join(lines) + "\n"


def read_source(source: str) -> str:
    if source == "-":
        return sys.stdin.read()
    try:
        return Path(source).read_text(encoding="utf-8")
    except OSError as error:
        raise SystemExit(f"summary.py: read {source}: {error}") from error


def write_dest(dest: str, markdown: str) -> None:
    if dest == "-":
        sys.stdout.write(markdown)
        return
    Path(dest).write_text(markdown, encoding="utf-8")


def stream(dest: Path, source: TextIO, sink: TextIO) -> None:
    """Copy the raw event stream to `dest`, echoing plain test output to `sink`.

    Lines that are not JSON (build failures, panics before the harness starts)
    are echoed verbatim so nothing the runner printed is lost.
    """
    with dest.open("w", encoding="utf-8") as raw:
        for line in source:
            raw.write(line)
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                sink.write(line)
                continue
            if isinstance(event, dict) and event.get("Action") == "output":
                output = event.get("Output")
                if isinstance(output, str):
                    sink.write(output)
            sink.flush()


def main(argv: list[str] | None = None) -> int:
    args = sys.argv[1:] if argv is None else argv
    if args and args[0] == "stream":
        if len(args) != 2:
            raise SystemExit("usage: summary.py stream FILE")
        stream(Path(args[1]), sys.stdin, sys.stdout)
        return 0
    source = args[0] if args else "-"
    dest = args[1] if len(args) > 1 else "-"
    write_dest(dest, render_summary(parse_events(read_source(source))))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
