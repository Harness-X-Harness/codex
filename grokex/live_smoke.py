#!/usr/bin/env python3
"""Run one bounded, secret-safe Grok scenario through a packaged App Server."""

import argparse
import base64
import hashlib
import json
import os
import queue
import shutil
import subprocess
import tarfile
import tempfile
import threading
import time
import tomllib
import uuid
from collections import deque
from pathlib import Path


SOURCE_ROOT = Path(__file__).resolve().parent
RELEASE_SOURCE = json.loads(
    (SOURCE_ROOT / "release-source.json").read_text(encoding="utf-8")
)
LIVE_CONTRACTS_PATH = SOURCE_ROOT / "live_contracts.json"
LIVE_CONTRACTS = json.loads(LIVE_CONTRACTS_PATH.read_text(encoding="utf-8"))
VERSION = RELEASE_SOURCE["version"]
TAG = f"grokex-v{VERSION}"
BASIC_SCENARIO = "basic-exact-reply"
CONTINUATION_SCENARIO = "encrypted-reasoning-tool-continuation"
COLLABORATION_SCENARIO = "ultra-full-history-collaboration"
IMAGE_SCENARIO = "image-generation-history-edit"
SCENARIOS = (
    BASIC_SCENARIO,
    CONTINUATION_SCENARIO,
    COLLABORATION_SCENARIO,
    IMAGE_SCENARIO,
)
STORY_BY_SCENARIO = {
    scenario: LIVE_CONTRACTS["scenarios"][scenario]["story"] for scenario in SCENARIOS
}
RELEASE_MODE = "release"
OBSERVATION_MODE = "observation"
MODES = (RELEASE_MODE, OBSERVATION_MODE)
# This module is the stream-based oracle; scenarios whose contract names
# another oracle are executed by grokex/validator (Go) instead.
STREAM_ORACLE = "app_server_stream"
BASIC_EXPECTED_AGENT_REPLY = "GROKEX_BASIC_RESPONSE_OK"
TOOL_NAME = "grokex_live_probe"
TOOL_OUTPUT_MARKER = "GROKEX_LIVE_TOOL_OK"
EXPECTED_AGENT_REPLY = "GROKEX_LIVE_RESPONSE_OK"
HISTORY_EXPECTED_AGENT_REPLY = TOOL_OUTPUT_MARKER
BASIC_TURN_SECONDS = LIVE_CONTRACTS["scenarios"][BASIC_SCENARIO]["turn_deadline_seconds"]
CONTINUATION_TURN_SECONDS = LIVE_CONTRACTS["scenarios"][CONTINUATION_SCENARIO][
    "turn_deadline_seconds"
]
COLLABORATION_TURN_SECONDS = LIVE_CONTRACTS["scenarios"][COLLABORATION_SCENARIO][
    "turn_deadline_seconds"
]
IMAGE_TURN_SECONDS = LIVE_CONTRACTS["scenarios"][IMAGE_SCENARIO]["turn_deadline_seconds"]
TERMINAL_RECONCILIATION_SECONDS = 5


class StageClock:
    """Record the lifecycle stages a scenario reaches and when it reached them."""

    def __init__(self) -> None:
        self.started = time.monotonic()
        self.current = "app_server_started"
        self.timings: list[dict[str, object]] = [{"elapsed_seconds": 0.0, "stage": self.current}]
        self.turn_started: float | None = None
        self.turn_durations_seconds: list[float] = []

    def mark(self, stage: str) -> None:
        self.current = stage
        self.timings.append(
            {"elapsed_seconds": round(time.monotonic() - self.started, 3), "stage": stage}
        )

    def start_turn(self) -> None:
        self.turn_started = time.monotonic()

    def finish_turn(self) -> None:
        if self.turn_started is not None:
            self.turn_durations_seconds.append(round(time.monotonic() - self.turn_started, 3))
            self.turn_started = None

    def evidence(self) -> dict[str, object]:
        durations = list(self.turn_durations_seconds)
        if self.turn_started is not None:
            durations.append(round(time.monotonic() - self.turn_started, 3))
        return {
            "stage_timings": self.timings,
            "turn_durations_seconds": durations,
        }


class LiveScenarioFailed(SystemExit):
    def __init__(
        self,
        waiting_for: str,
        last_stage: dict[str, object],
        failure_category: str,
        outcome: str,
    ) -> None:
        super().__init__(f"Live scenario failed while waiting for {waiting_for}")
        self.last_stage = {
            **last_stage,
            "does_not_prove": "product_root_cause",
            "failure_category": failure_category,
            "outcome": outcome,
        }


class LiveDeadlineExpired(LiveScenarioFailed):
    def __init__(self, waiting_for: str, last_stage: dict[str, object]) -> None:
        super().__init__(waiting_for, last_stage, "deadline", "deadline_expired")


class LiveProofIncomplete(LiveScenarioFailed):
    def __init__(self, waiting_for: str, last_stage: dict[str, object]) -> None:
        super().__init__(waiting_for, last_stage, "oracle_insufficient", "not_proven")


class LiveObservationFailed(LiveScenarioFailed):
    def __init__(self, waiting_for: str, last_stage: dict[str, object]) -> None:
        super().__init__(
            waiting_for,
            last_stage,
            "app_server_observation",
            "observation_failed",
        )


class LiveImageDecoderFailed(LiveScenarioFailed):
    def __init__(self, waiting_for: str, last_stage: dict[str, object]) -> None:
        super().__init__(
            waiting_for,
            last_stage,
            "image_decoder_observation",
            "observation_failed",
        )


class ImageDecoderObservationError(RuntimeError):
    pass


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def extract_archive(path: Path, destination: Path) -> Path:
    with tarfile.open(path, "r:gz") as archive:
        members = archive.getmembers()
        for member in members:
            candidate = Path(member.name)
            if candidate.is_absolute() or ".." in candidate.parts or member.issym() or member.islnk():
                raise SystemExit("release archive contains an unsafe member")
        archive.extractall(destination, members=members, filter="data")
    roots = {Path(member.name).parts[0] for member in members if member.name}
    if len(roots) != 1:
        raise SystemExit("release archive does not have a single root")
    root = destination / roots.pop()
    if not root.name.startswith("grokex-v") or not root.is_dir():
        raise SystemExit("release archive root is missing")
    return root


class AppServer:
    def __init__(
        self,
        binary: Path,
        codex_home: Path,
        workspace: Path,
        redaction: str,
    ) -> None:
        environment = os.environ.copy()
        environment["CODEX_HOME"] = str(codex_home)
        environment["NO_COLOR"] = "1"
        self.redaction = redaction
        self.stderr_tail: deque[str] = deque(maxlen=8)
        self.process = subprocess.Popen(
            [str(binary), "app-server", "--strict-config", "--listen", "stdio://"],
            cwd=workspace,
            env=environment,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self.messages: queue.Queue[dict[str, object]] = queue.Queue()
        self.deferred_messages: deque[dict[str, object]] = deque()
        self.reader = threading.Thread(target=self._read_stdout, daemon=True)
        self.stderr_reader = threading.Thread(target=self._read_stderr, daemon=True)
        self.reader.start()
        self.stderr_reader.start()

    def _read_stdout(self) -> None:
        assert self.process.stdout is not None
        for line in self.process.stdout:
            try:
                value = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(value, dict):
                self.messages.put(value)

    def _read_stderr(self) -> None:
        assert self.process.stderr is not None
        for line in self.process.stderr:
            sanitized = line.rstrip().replace(self.redaction, "<redacted>")
            if sanitized:
                self.stderr_tail.append(sanitized[:500])

    def send(self, message: dict[str, object]) -> None:
        if self.process.poll() is not None:
            raise SystemExit("App Server exited before request submission")
        assert self.process.stdin is not None
        self.process.stdin.write(json.dumps(message, separators=(",", ":")) + "\n")
        self.process.stdin.flush()

    def request(
        self,
        request_id: int,
        method: str,
        params: dict[str, object],
        deadline: float | None = None,
    ) -> dict[str, object]:
        self.send({"id": request_id, "method": method, "params": params})
        request_deadline = time.monotonic() + 30
        if deadline is not None:
            request_deadline = min(request_deadline, deadline)
        while time.monotonic() < request_deadline:
            message = self._next_incoming_message(request_deadline, method)
            if message.get("id") != request_id or "method" in message:
                self.deferred_messages.append(message)
                continue
            if "error" in message:
                raise SystemExit(f"App Server rejected {method}")
            response = message.get("response", message.get("result"))
            if not isinstance(response, dict):
                raise SystemExit(f"App Server returned an invalid {method} response")
            return response
        raise LiveDeadlineExpired(method, {})

    def next_message(self, deadline: float, waiting_for: str) -> dict[str, object]:
        if self.deferred_messages:
            return self.deferred_messages.popleft()
        return self._next_incoming_message(deadline, waiting_for)

    def _next_incoming_message(
        self, deadline: float, waiting_for: str
    ) -> dict[str, object]:
        remaining = max(0.0, deadline - time.monotonic())
        try:
            return self.messages.get(timeout=remaining)
        except queue.Empty as error:
            status = self.process.poll()
            if status is not None:
                details = ""
                if waiting_for == "initialize":
                    self.stderr_reader.join(timeout=1)
                    if self.stderr_tail:
                        details = "\n" + "\n".join(self.stderr_tail)
                raise SystemExit(
                    f"App Server exited with status {status} while waiting for {waiting_for}"
                    f"{details}"
                ) from error
            raise LiveDeadlineExpired(waiting_for, {}) from error

    def close(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)


def terminal_agent_reply(turn: object, turn_id: str) -> object:
    if not isinstance(turn, dict) or turn.get("id") != turn_id:
        return None
    items = turn.get("items")
    if not isinstance(items, list):
        return None
    for item in reversed(items):
        if not isinstance(item, dict) or item.get("type") != "agentMessage":
            continue
        if item.get("phase") not in (None, "final_answer"):
            continue
        if item.get("delivery") is not None:
            continue
        return item.get("text")
    return None


def read_thread_snapshot(
    server: AppServer,
    request_id: int,
    thread_id: str,
    deadline: float,
) -> dict[str, object]:
    if time.monotonic() >= deadline:
        raise LiveDeadlineExpired("thread/read", {})
    response = server.request(
        request_id,
        "thread/read",
        {"threadId": thread_id},
        deadline,
    )
    thread = response.get("thread")
    if not isinstance(thread, dict) or thread.get("id") != thread_id:
        raise SystemExit("App Server returned an invalid thread/read response")
    if time.monotonic() >= deadline:
        raise LiveDeadlineExpired("thread/turns/list", {})
    turns_response = server.request(
        request_id + 1,
        "thread/turns/list",
        {
            "itemsView": "full",
            "limit": 50,
            "sortDirection": "desc",
            "threadId": thread_id,
        },
        deadline,
    )
    turns = turns_response.get("data")
    if not isinstance(turns, list):
        raise SystemExit("App Server returned an invalid thread/turns/list response")
    thread["turns"] = turns
    return thread


def snapshot_turn(thread: object, turn_id: str) -> dict[str, object] | None:
    if not isinstance(thread, dict):
        return None
    turns = thread.get("turns")
    if not isinstance(turns, list):
        return None
    for turn in turns:
        if isinstance(turn, dict) and turn.get("id") == turn_id:
            return turn
    return None


def require_turn_start_identity(
    response: dict[str, object],
    turn_name: str,
    excluded_turn_ids: frozenset[str] = frozenset(),
) -> str:
    turn = response.get("turn")
    turn_id = turn.get("id") if isinstance(turn, dict) else None
    if (
        not isinstance(turn_id, str)
        or not turn_id
        or turn_id in excluded_turn_ids
    ):
        raise LiveScenarioFailed(
            f"{turn_name} identity",
            {"last_proven_stage": "turn_start_response_received"},
            "semantic_contract",
            "semantic_failure",
        )
    return turn_id


def terminal_reply_matches(terminal_reply: object, expected_reply: str | None) -> bool:
    if expected_reply is None:
        return isinstance(terminal_reply, str) and bool(terminal_reply.strip())
    return terminal_reply == expected_reply


def wait_for_terminal_reply(
    server: AppServer,
    deadline: float,
    thread_id: str,
    turn_id: str,
    expected_reply: str | None,
    turn_name: str,
) -> dict[str, str]:
    terminal_reply = None
    terminal_seen = False
    status = None

    while time.monotonic() < deadline:
        try:
            message = server.next_message(deadline, turn_name)
        except LiveDeadlineExpired as error:
            raise LiveDeadlineExpired(
                turn_name,
                terminal_reply_last_stage(status, terminal_reply, expected_reply),
            ) from error
        method = message.get("method")
        params = message.get("params")
        if (
            method != "turn/completed"
            or not isinstance(params, dict)
            or params.get("threadId") != thread_id
        ):
            continue
        turn = params.get("turn")
        if not isinstance(turn, dict) or turn.get("id") != turn_id:
            continue
        terminal_seen = True
        status = turn.get("status")
        terminal_reply = terminal_agent_reply(turn, turn_id)
        break

    if not terminal_seen:
        raise LiveDeadlineExpired(
            turn_name,
            terminal_reply_last_stage(status, terminal_reply, expected_reply),
        )
    if status != "completed":
        raise LiveScenarioFailed(
            turn_name,
            terminal_reply_last_stage(status, terminal_reply, expected_reply),
            "semantic_contract",
            "semantic_failure",
        )
    if not terminal_reply_matches(terminal_reply, expected_reply):
        raise LiveScenarioFailed(
            turn_name,
            terminal_reply_last_stage(status, terminal_reply, expected_reply),
            "semantic_contract",
            "semantic_failure",
        )
    return {
        "response_assertion": (
            "nonempty_agent_message" if expected_reply is None else "exact_match"
        ),
        "status": status,
    }


def terminal_reply_last_stage(
    status: object, terminal_reply: object, expected_reply: str | None
) -> dict[str, object]:
    reply_matches = terminal_reply_matches(terminal_reply, expected_reply)
    if status == "completed" and reply_matches:
        last_proven_stage = "completed"
    elif status == "completed":
        last_proven_stage = "turn_completed"
    else:
        last_proven_stage = "no_events"
    return {
        "agent_reply_seen": isinstance(terminal_reply, str),
        "agent_reply_matches": reply_matches,
        "last_proven_stage": last_proven_stage,
        "turn_status": status if isinstance(status, str) else "missing",
    }


def wait_for_basic_turn(
    server: AppServer,
    deadline: float,
    thread_id: str,
    turn_id: str,
) -> dict[str, str]:
    return wait_for_terminal_reply(
        server,
        deadline,
        thread_id,
        turn_id,
        None,
        "the single basic Grok Turn",
    )


def wait_for_verified_turn(
    server: AppServer,
    deadline: float,
    thread_id: str,
    turn_id: str,
) -> dict[str, object]:
    reasoning_completed = False
    encrypted_reasoning_seen = False
    tool_request_count = 0
    tool_completed = False
    terminal_reply = None
    terminal_seen = False
    status = None

    while time.monotonic() < deadline:
        try:
            message = server.next_message(deadline, "the single Grok Turn")
        except LiveDeadlineExpired as error:
            raise LiveDeadlineExpired(
                "the single Grok Turn",
                verified_turn_last_stage(
                    status=status,
                    reasoning_completed=reasoning_completed,
                    encrypted_reasoning_seen=encrypted_reasoning_seen,
                    tool_request_count=tool_request_count,
                    tool_completed=tool_completed,
                    terminal_reply=terminal_reply,
                ),
            ) from error
        method = message.get("method")
        params = message.get("params")
        if (
            method == "rawResponseItem/completed"
            and isinstance(params, dict)
            and params.get("threadId") == thread_id
            and params.get("turnId") == turn_id
        ):
            item = params.get("item")
            encrypted_reasoning_seen = encrypted_reasoning_seen or (
                isinstance(item, dict)
                and item.get("type") == "reasoning"
                and isinstance(item.get("encrypted_content"), str)
                and bool(item["encrypted_content"])
            )
            continue
        if method == "item/tool/call":
            if (
                not isinstance(params, dict)
                or params.get("threadId") != thread_id
                or params.get("turnId") != turn_id
            ):
                continue
            request_id = message.get("id")
            if not isinstance(request_id, (int, str)) or params.get("tool") != TOOL_NAME:
                last_stage = verified_turn_last_stage(
                    status=status,
                    reasoning_completed=reasoning_completed,
                    encrypted_reasoning_seen=encrypted_reasoning_seen,
                    tool_request_count=tool_request_count,
                    tool_completed=tool_completed,
                    terminal_reply=terminal_reply,
                )
                last_stage["last_proven_stage"] = "unexpected_tool_request_seen"
                raise LiveScenarioFailed(
                    "the single Grok Turn",
                    last_stage,
                    "app_server_protocol",
                    "evidence_insufficient",
                )
            if params.get("arguments") != {}:
                last_stage = verified_turn_last_stage(
                    status=status,
                    reasoning_completed=reasoning_completed,
                    encrypted_reasoning_seen=encrypted_reasoning_seen,
                    tool_request_count=tool_request_count,
                    tool_completed=tool_completed,
                    terminal_reply=terminal_reply,
                )
                last_stage["last_proven_stage"] = "invalid_tool_arguments_seen"
                raise LiveScenarioFailed(
                    "the single Grok Turn",
                    last_stage,
                    "semantic_contract",
                    "semantic_failure",
                )
            tool_request_count += 1
            server.send(
                {
                    "id": request_id,
                    "result": {
                        "contentItems": [
                            {"type": "inputText", "text": TOOL_OUTPUT_MARKER}
                        ],
                        "success": True,
                    },
                }
            )
            continue
        if not isinstance(params, dict) or params.get("threadId") != thread_id:
            continue
        if method == "item/completed":
            if params.get("turnId") != turn_id:
                continue
            item = params.get("item")
            if not isinstance(item, dict):
                continue
            item_type = item.get("type")
            if item_type == "reasoning":
                reasoning_completed = True
            elif item_type == "dynamicToolCall":
                content_items = item.get("contentItems")
                tool_completed = tool_completed or (
                    item.get("tool") == TOOL_NAME
                    and item.get("status") == "completed"
                    and item.get("success") is True
                    and isinstance(content_items, list)
                    and any(
                        isinstance(content, dict)
                        and content.get("type") == "inputText"
                        and content.get("text") == TOOL_OUTPUT_MARKER
                        for content in content_items
                    )
                )
        elif method == "turn/completed":
            turn = params.get("turn")
            if not isinstance(turn, dict) or turn.get("id") != turn_id:
                continue
            terminal_seen = True
            status = turn.get("status")
            terminal_reply = terminal_agent_reply(turn, turn_id)
            break

    if not terminal_seen:
        raise LiveDeadlineExpired(
            "the single Grok Turn",
            verified_turn_last_stage(
                status=status,
                reasoning_completed=reasoning_completed,
                encrypted_reasoning_seen=encrypted_reasoning_seen,
                tool_request_count=tool_request_count,
                tool_completed=tool_completed,
                terminal_reply=terminal_reply,
            ),
        )
    failed = status != "completed" or not (
        reasoning_completed
        and encrypted_reasoning_seen
        and tool_request_count >= 1
        and tool_completed
        and terminal_reply == EXPECTED_AGENT_REPLY
    )
    if failed:
        raise LiveScenarioFailed(
            "the single Grok Turn",
            verified_turn_last_stage(
                status=status,
                reasoning_completed=reasoning_completed,
                encrypted_reasoning_seen=encrypted_reasoning_seen,
                tool_request_count=tool_request_count,
                tool_completed=tool_completed,
                terminal_reply=terminal_reply,
            ),
            "semantic_contract",
            "semantic_failure",
        )
    return {
        "encrypted_reasoning_observed": True,
        "response_assertion": "exact_match",
        "status": status,
        "tool_continuation": "completed",
        "tool_request_count": tool_request_count,
    }


def verified_turn_last_stage(
    *,
    status: object,
    reasoning_completed: bool,
    encrypted_reasoning_seen: bool,
    tool_request_count: int,
    tool_completed: bool,
    terminal_reply: object,
) -> dict[str, object]:
    response_matches = terminal_reply == EXPECTED_AGENT_REPLY
    if (
        status == "completed"
        and reasoning_completed
        and encrypted_reasoning_seen
        and tool_request_count
        and tool_completed
        and response_matches
    ):
        last_proven_stage = "completed"
    elif status == "completed":
        last_proven_stage = "turn_completed"
    elif tool_completed:
        last_proven_stage = "tool_completed"
    elif tool_request_count:
        last_proven_stage = "tool_requested"
    elif reasoning_completed or encrypted_reasoning_seen:
        last_proven_stage = "reasoning_seen"
    else:
        last_proven_stage = "no_events"
    return {
        "agent_reply_seen": isinstance(terminal_reply, str),
        "encrypted_reasoning_seen": encrypted_reasoning_seen,
        "last_proven_stage": last_proven_stage,
        "reasoning_completed": reasoning_completed,
        "response_matches": response_matches,
        "tool_completed": tool_completed,
        "tool_request_count": tool_request_count,
        "turn_status": status if isinstance(status, str) else "missing",
    }


def is_full_history_spawn(parsed_arguments: dict[str, object]) -> bool:
    fork_turns = parsed_arguments.get("fork_turns")
    if fork_turns is None:
        return True
    if not isinstance(fork_turns, str):
        return False
    fork_turns = fork_turns.strip()
    return fork_turns == "" or fork_turns.casefold() == "all"


def is_default_full_history_spawn(parsed_arguments: dict[str, object]) -> bool:
    if not is_full_history_spawn(parsed_arguments):
        return False
    agent_type = parsed_arguments.get("agent_type")
    if isinstance(agent_type, str):
        agent_type = agent_type.strip()
    return agent_type in (None, "") and parsed_arguments.get("model") is None


def is_canonical_uuid_v4(value: object) -> bool:
    if not isinstance(value, str):
        return False
    try:
        parsed = uuid.UUID(value)
    except ValueError:
        return False
    return parsed.version == 4 and str(parsed) == value


def classify_collaboration_stage(
    *,
    root_status: str | None,
    parent_reply_seen: bool,
    default_spawn_count: int,
    default_child_count: int,
    completed_default_child_count: int,
) -> str:
    if (
        root_status == "completed"
        and completed_default_child_count
        and parent_reply_seen
        and default_spawn_count
        and default_child_count
    ):
        return "completed"
    if root_status in {"failed", "interrupted"}:
        return "parent_turn_terminal_failure"
    if parent_reply_seen:
        return "parent_reply_seen"
    if root_status == "completed":
        return "parent_turn_completed"
    if completed_default_child_count:
        return "child_completed"
    if default_child_count:
        return "child_created"
    if default_spawn_count:
        return "default_spawn_requested"
    return "no_semantic_proof"


def collaboration_last_stage(
    *,
    root_status: str | None,
    child_thread_ids: set[str],
    default_child_thread_ids: set[str],
    child_completed_thread_ids: set[str],
    child_activity_completed_thread_ids: set[str],
    child_reply_thread_ids: set[str],
    default_spawn_call_ids: set[str],
    explicit_fork_spawn_call_ids: set[str],
    overridden_spawn_call_ids: set[str],
    failed_tool_call_ids: set[str],
    missing_spawn_identity_count: int,
    parent_terminal_result_seen: bool,
    response_counts: dict[str, int],
    spawn_started_thread_ids: set[str],
    wait_completed_call_ids: set[str],
    unexpected_tool_call_ids: set[str],
    matching_child_thread_ids: set[str],
    root_snapshot_available: bool,
    child_snapshot_thread_ids: set[str],
    child_parent_link_thread_ids: set[str],
    child_provider_match_thread_ids: set[str],
    child_model_contract_thread_ids: set[str],
) -> dict[str, object]:
    completed_default_child_count = len(matching_child_thread_ids)
    return {
        "child_activity_completed_count": len(child_activity_completed_thread_ids),
        "child_completed_count": len(child_completed_thread_ids),
        "child_count": len(child_thread_ids),
        "child_model_contract_count": len(child_model_contract_thread_ids),
        "child_parent_link_match_count": len(child_parent_link_thread_ids),
        "child_provider_match_count": len(child_provider_match_thread_ids),
        "child_reply_count": len(child_reply_thread_ids),
        "child_snapshot_available_count": len(child_snapshot_thread_ids),
        "completed_default_child_count": completed_default_child_count,
        "default_child_count": len(default_child_thread_ids),
        "default_spawn_count": len(default_spawn_call_ids),
        "explicit_fork_spawn_count": len(explicit_fork_spawn_call_ids),
        "overridden_spawn_count": len(overridden_spawn_call_ids),
        "failed_collaboration_tool_count": len(failed_tool_call_ids),
        "last_proven_stage": classify_collaboration_stage(
            root_status=root_status,
            parent_reply_seen=parent_terminal_result_seen,
            default_spawn_count=len(default_spawn_call_ids),
            default_child_count=len(default_child_thread_ids),
            completed_default_child_count=completed_default_child_count,
        ),
        "missing_spawn_identity_count": missing_spawn_identity_count,
        "parent_terminal_result_seen": parent_terminal_result_seen,
        "provider_response_count": sum(response_counts.values()),
        "root_status": root_status,
        "root_snapshot_available": root_snapshot_available,
        "semantic_result_match_count": len(matching_child_thread_ids),
        "spawn_count": len(spawn_started_thread_ids),
        "unexpected_collaboration_tool_count": len(unexpected_tool_call_ids),
        "wait_count": len(wait_completed_call_ids),
    }


def wait_for_collaboration_turn(
    server: AppServer,
    deadline: float,
    root_thread_id: str,
    root_turn_id: str,
) -> tuple[dict[str, object], frozenset[str]]:
    stream_deadline = max(
        time.monotonic(), deadline - TERMINAL_RECONCILIATION_SECONDS
    )
    snapshot_schedule = deque(
        [
            stream_deadline,
            max(
                stream_deadline,
                deadline - (TERMINAL_RECONCILIATION_SECONDS / 2),
            ),
        ]
    )
    child_activity_completed_thread_ids: set[str] = set()
    child_completed_thread_ids: set[str] = set()
    child_terminal_replies: dict[str, set[str]] = {}
    child_thread_ids: set[str] = set()
    default_child_thread_ids: set[str] = set()
    default_spawn_call_ids: set[str] = set()
    raw_spawn_arguments_by_call_id: dict[str, dict[str, object]] = {}
    persisted_spawn_children_by_call_id: dict[str, str] = {}
    explicit_fork_spawn_call_ids: set[str] = set()
    overridden_spawn_call_ids: set[str] = set()
    missing_spawn_identity_count = 0
    response_counts: dict[str, int] = {}
    root_status: str | None = None
    root_terminal_reply: object = None
    spawn_started_thread_ids: set[str] = set()
    wait_completed_call_ids: set[str] = set()
    failed_tool_call_ids: set[str] = set()
    unexpected_tool_call_ids: set[str] = set()
    child_snapshot_thread_ids: set[str] = set()
    child_parent_link_thread_ids: set[str] = set()
    child_provider_match_thread_ids: set[str] = set()
    child_model_contract_thread_ids: set[str] = set()
    child_contract_failure_thread_ids: set[str] = set()
    child_snapshot_failure_count = 0
    root_snapshot_available = False
    root_terminal_snapshot_available = False
    terminal_refresh_needed = False
    next_snapshot_request_id = 100

    def associate_spawn(call_id: str) -> bool:
        arguments = raw_spawn_arguments_by_call_id.get(call_id)
        child_thread_id = persisted_spawn_children_by_call_id.get(call_id)
        if arguments is None or child_thread_id is None:
            return False
        if not is_full_history_spawn(arguments):
            explicit_fork_spawn_call_ids.add(call_id)
            return False
        if not is_default_full_history_spawn(arguments):
            overridden_spawn_call_ids.add(call_id)
            return False
        is_new = child_thread_id not in default_child_thread_ids
        default_spawn_call_ids.add(call_id)
        child_thread_ids.add(child_thread_id)
        default_child_thread_ids.add(child_thread_id)
        child_model_contract_thread_ids.add(child_thread_id)
        return is_new

    def observe_root_item(item: object, *, persisted: bool = False) -> None:
        if not isinstance(item, dict):
            return
        if item.get("type") == "subAgentActivity":
            agent_thread_id = item.get("agentThreadId")
            kind = item.get("kind")
            if isinstance(agent_thread_id, str) and agent_thread_id:
                child_thread_ids.add(agent_thread_id)
                if kind == "started":
                    if not persisted:
                        spawn_started_thread_ids.add(agent_thread_id)
                    call_id = item.get("id")
                    if persisted and isinstance(call_id, str) and call_id:
                        persisted_spawn_children_by_call_id[call_id] = agent_thread_id
                        associate_spawn(call_id)
                elif kind == "completed":
                    child_activity_completed_thread_ids.add(agent_thread_id)
            return
        if item.get("type") != "collabAgentToolCall":
            return
        call_id = item.get("id")
        if not isinstance(call_id, str) or not call_id:
            return
        status = item.get("status")
        if status in {"failed", "interrupted"}:
            failed_tool_call_ids.add(call_id)
            return
        if status != "completed":
            return
        tool = item.get("tool")
        if tool == "wait":
            wait_completed_call_ids.add(call_id)
        else:
            unexpected_tool_call_ids.add(call_id)

    def observe_root_turn(turn: object, *, persisted: bool = False) -> None:
        nonlocal root_status, root_terminal_reply, terminal_refresh_needed
        if not isinstance(turn, dict) or turn.get("id") != root_turn_id:
            return
        observed_status = turn.get("status")
        observed_status = observed_status if isinstance(observed_status, str) else None
        if observed_status == "completed":
            root_status = observed_status
            root_terminal_reply = terminal_agent_reply(turn, root_turn_id)
            if not persisted:
                terminal_refresh_needed = True
        elif observed_status in {"failed", "interrupted"}:
            root_status = observed_status
            root_terminal_reply = None
        elif root_status is None:
            root_status = observed_status
        items = turn.get("items")
        if isinstance(items, list):
            for item in items:
                observe_root_item(item, persisted=persisted)

    def observe_child_turn(
        child_thread_id: str,
        turn: object,
        *,
        event: bool = False,
    ) -> None:
        nonlocal terminal_refresh_needed
        if not isinstance(turn, dict) or turn.get("status") != "completed":
            return
        turn_id = turn.get("id")
        if not isinstance(turn_id, str) or not turn_id:
            return
        child_completed_thread_ids.add(child_thread_id)
        if event:
            terminal_refresh_needed = True
        reply = terminal_agent_reply(turn, turn_id)
        if isinstance(reply, str) and is_canonical_uuid_v4(reply):
            child_terminal_replies.setdefault(child_thread_id, set()).add(reply)

    def matching_child_thread_ids() -> set[str]:
        if not is_canonical_uuid_v4(root_terminal_reply):
            return set()
        required = (
            default_child_thread_ids
            & child_completed_thread_ids
            & child_snapshot_thread_ids
            & child_parent_link_thread_ids
            & child_provider_match_thread_ids
            & child_model_contract_thread_ids
        )
        return {
            child_thread_id
            for child_thread_id in required
            if root_terminal_reply in child_terminal_replies.get(child_thread_id, set())
        }

    def semantic_failure_proven() -> bool:
        if child_contract_failure_thread_ids:
            return True
        if root_status != "completed":
            return False
        if root_terminal_reply is not None and not is_canonical_uuid_v4(
            root_terminal_reply
        ):
            return True
        return root_terminal_snapshot_available and not is_canonical_uuid_v4(
            root_terminal_reply
        )

    def child_reply_thread_ids() -> set[str]:
        return {
            child_thread_id
            for child_thread_id, replies in child_terminal_replies.items()
            if replies
        }

    def current_last_stage() -> dict[str, object]:
        matching = matching_child_thread_ids()
        last_stage = collaboration_last_stage(
            root_status=root_status,
            child_thread_ids=child_thread_ids,
            default_child_thread_ids=default_child_thread_ids,
            child_completed_thread_ids=child_completed_thread_ids,
            child_activity_completed_thread_ids=child_activity_completed_thread_ids,
            child_reply_thread_ids=child_reply_thread_ids(),
            default_spawn_call_ids=default_spawn_call_ids,
            explicit_fork_spawn_call_ids=explicit_fork_spawn_call_ids,
            overridden_spawn_call_ids=overridden_spawn_call_ids,
            failed_tool_call_ids=failed_tool_call_ids,
            missing_spawn_identity_count=missing_spawn_identity_count,
            parent_terminal_result_seen=(
                root_status == "completed" and is_canonical_uuid_v4(root_terminal_reply)
            ),
            response_counts=response_counts,
            spawn_started_thread_ids=spawn_started_thread_ids,
            wait_completed_call_ids=wait_completed_call_ids,
            unexpected_tool_call_ids=unexpected_tool_call_ids,
            matching_child_thread_ids=matching,
            root_snapshot_available=root_snapshot_available,
            child_snapshot_thread_ids=child_snapshot_thread_ids,
            child_parent_link_thread_ids=child_parent_link_thread_ids,
            child_provider_match_thread_ids=child_provider_match_thread_ids,
            child_model_contract_thread_ids=child_model_contract_thread_ids,
        )
        last_stage["root_terminal_snapshot_available"] = (
            root_terminal_snapshot_available
        )
        last_stage["child_snapshot_failure_count"] = child_snapshot_failure_count
        last_stage["child_contract_failure_count"] = len(
            child_contract_failure_thread_ids
        )
        last_stage["semantic_failure_proven"] = semantic_failure_proven()
        return last_stage

    def raise_terminal_outcome(cause: BaseException | None = None) -> None:
        last_stage = current_last_stage()
        if root_status in {"failed", "interrupted"} or semantic_failure_proven():
            failure: BaseException = LiveScenarioFailed(
                "the Grok Ultra collaboration Turn",
                last_stage,
                "semantic_contract",
                "semantic_failure",
            )
        elif child_snapshot_failure_count:
            failure = LiveObservationFailed(
                "a Grok Ultra child Thread snapshot",
                last_stage,
            )
        elif root_status == "completed":
            failure = LiveProofIncomplete(
                "the Grok Ultra collaboration Turn",
                last_stage,
            )
        else:
            failure = LiveDeadlineExpired(
                "the Grok Ultra collaboration Turn",
                last_stage,
            )
        if cause is None:
            raise failure
        raise failure from cause

    def reconcile(read_deadline: float) -> None:
        nonlocal child_snapshot_failure_count
        nonlocal next_snapshot_request_id, root_snapshot_available
        nonlocal root_terminal_snapshot_available
        request_id = next_snapshot_request_id
        next_snapshot_request_id += 2
        try:
            root_thread = read_thread_snapshot(
                server,
                request_id,
                root_thread_id,
                read_deadline,
            )
        except LiveDeadlineExpired:
            raise
        except SystemExit as error:
            raise LiveObservationFailed(
                "the Grok Ultra parent Thread snapshot",
                current_last_stage(),
            ) from error
        if root_thread is not None:
            root_turn = snapshot_turn(root_thread, root_turn_id)
            if root_turn is not None:
                root_snapshot_available = True
                root_terminal_snapshot_available = (
                    root_terminal_snapshot_available
                    or root_turn.get("status") == "completed"
                )
                observe_root_turn(root_turn, persisted=True)
        candidates = sorted(
            default_child_thread_ids,
            key=lambda child_thread_id: (
                root_terminal_reply
                not in child_terminal_replies.get(child_thread_id, set()),
                child_thread_id not in child_completed_thread_ids,
                child_thread_id not in child_terminal_replies,
                child_thread_id,
            ),
        )
        for index, child_thread_id in enumerate(candidates):
            remaining_candidates = len(candidates) - index
            remaining_seconds = max(0.0, read_deadline - time.monotonic())
            child_deadline = min(
                read_deadline,
                time.monotonic() + (remaining_seconds / remaining_candidates),
            )
            try:
                child_thread = read_thread_snapshot(
                    server,
                    next_snapshot_request_id,
                    child_thread_id,
                    child_deadline,
                )
            except LiveDeadlineExpired:
                child_thread = None
            except SystemExit:
                child_snapshot_failure_count += 1
                child_thread = None
            finally:
                next_snapshot_request_id += 2
            if child_thread is None:
                continue
            child_snapshot_thread_ids.add(child_thread_id)
            if child_thread.get("parentThreadId") == root_thread_id:
                child_parent_link_thread_ids.add(child_thread_id)
            else:
                child_contract_failure_thread_ids.add(child_thread_id)
            if child_thread.get("modelProvider") == "grok":
                child_provider_match_thread_ids.add(child_thread_id)
            else:
                child_contract_failure_thread_ids.add(child_thread_id)
            turns = child_thread.get("turns")
            if isinstance(turns, list):
                for turn in turns:
                    observe_child_turn(child_thread_id, turn)
            if matching_child_thread_ids():
                break

    while time.monotonic() < deadline:
        if root_status == "completed" and matching_child_thread_ids():
            break
        if terminal_refresh_needed:
            terminal_refresh_needed = False
            try:
                reconcile(snapshot_schedule[0] if snapshot_schedule else deadline)
            except LiveDeadlineExpired:
                pass
            if matching_child_thread_ids():
                break
        while (
            snapshot_schedule
            and time.monotonic() >= snapshot_schedule[0]
            and time.monotonic() < deadline
        ):
            snapshot_schedule.popleft()
            try:
                reconcile(snapshot_schedule[0] if snapshot_schedule else deadline)
            except LiveDeadlineExpired:
                pass
            if matching_child_thread_ids():
                break
        if matching_child_thread_ids():
            break
        message_deadline = snapshot_schedule[0] if snapshot_schedule else deadline
        try:
            message = server.next_message(
                message_deadline,
                "the Grok Ultra collaboration Turn",
            )
        except LiveDeadlineExpired as error:
            if snapshot_schedule and time.monotonic() < deadline:
                snapshot_schedule.popleft()
                try:
                    reconcile(snapshot_schedule[0] if snapshot_schedule else deadline)
                except LiveDeadlineExpired:
                    pass
                if matching_child_thread_ids():
                    break
                continue
            raise_terminal_outcome(error)
        method = message.get("method")
        params = message.get("params")
        if not isinstance(params, dict):
            continue
        thread_id = params.get("threadId")

        if method == "rawResponse/completed" and isinstance(thread_id, str):
            response_counts[thread_id] = response_counts.get(thread_id, 0) + 1
            continue

        if (
            method == "rawResponseItem/completed"
            and thread_id == root_thread_id
            and params.get("turnId") == root_turn_id
        ):
            item = params.get("item")
            if not isinstance(item, dict) or item.get("type") != "function_call":
                continue
            arguments = item.get("arguments")
            if not isinstance(arguments, str):
                continue
            try:
                parsed_arguments = json.loads(arguments)
            except json.JSONDecodeError:
                continue
            if not isinstance(parsed_arguments, dict):
                continue
            call_id = item.get("call_id")
            if not isinstance(call_id, str) or not call_id:
                missing_spawn_identity_count += 1
                continue
            raw_spawn_arguments_by_call_id[call_id] = parsed_arguments
            if associate_spawn(call_id) and root_snapshot_available:
                terminal_refresh_needed = True
            continue

        if (
            method == "item/completed"
            and thread_id == root_thread_id
            and params.get("turnId") == root_turn_id
        ):
            item = params.get("item")
            observe_root_item(item)
            continue

        if method == "turn/completed":
            turn = params.get("turn")
            if not isinstance(turn, dict):
                continue
            event_turn_id = turn.get("id")
            status = turn.get("status")
            if thread_id == root_thread_id:
                if event_turn_id != root_turn_id:
                    continue
                observe_root_turn(turn)
                if status != "completed":
                    raise LiveScenarioFailed(
                        "the Grok Ultra collaboration Turn",
                        current_last_stage(),
                        "semantic_contract",
                        "semantic_failure",
                    )
            elif isinstance(thread_id, str) and isinstance(event_turn_id, str):
                observe_child_turn(thread_id, turn, event=True)

    last_stage = current_last_stage()
    if semantic_failure_proven():
        raise_terminal_outcome()
    if last_stage["last_proven_stage"] != "completed":
        raise_terminal_outcome()
    if root_status != "completed":
        raise LiveScenarioFailed(
            "the Grok Ultra collaboration Turn",
            last_stage,
            "semantic_contract",
            "semantic_failure",
        )
    completed_children = matching_child_thread_ids()
    if not completed_children:
        raise LiveScenarioFailed(
            "the Grok Ultra collaboration Turn",
            last_stage,
            "semantic_contract",
            "semantic_failure",
        )
    if not default_spawn_call_ids or not default_child_thread_ids:
        raise LiveScenarioFailed(
            "the Grok Ultra collaboration Turn",
            last_stage,
            "semantic_contract",
            "semantic_failure",
        )
    provider_response_count = sum(response_counts.values())
    return ({
        "child_completion": "completed",
        "child_parent_link_verified": True,
        "child_count": len(child_thread_ids),
        "child_model_evidence": "parent_model_default_spawn_and_stock_inheritance",
        "child_model_verified": True,
        "child_provider_binding": "grok/grok-4.6",
        "child_provider_verified": True,
        "child_response_assertion": "canonical_uuid_v4",
        "default_full_history": "completed",
        "evidence_source": "public_snapshot_and_stream",
        "explicit_fork_spawn_count": len(explicit_fork_spawn_call_ids),
        "overridden_spawn_count": len(overridden_spawn_call_ids),
        "failed_collaboration_tool_count": len(failed_tool_call_ids),
        "missing_spawn_identity_count": missing_spawn_identity_count,
        "parent_completion": "completed",
        "provider_response_count": provider_response_count,
        "response_assertion": "child_echo_match",
        "spawn_count": len(spawn_started_thread_ids),
        "status": root_status,
        "unexpected_collaboration_tool_count": len(unexpected_tool_call_ids),
        "wait_count": len(wait_completed_call_ids),
        "result_delivery": "completed",
        "result_delivery_verified": True,
    }, frozenset(completed_children))


def classify_image_stage(
    *,
    completed: int,
    failed: int,
    agent_reply_seen: bool,
    history_args_seen: bool,
    require_history: bool,
    image_function_call_count: int,
    turn_status: str | None,
) -> str:
    if (
        turn_status == "completed"
        and completed
        and agent_reply_seen
        and (history_args_seen or not require_history)
    ):
        return "completed"
    if turn_status == "completed":
        return "turn_completed"
    if turn_status in {"failed", "interrupted"}:
        return "turn_terminal_failure"
    if completed and agent_reply_seen:
        return "image_and_agent_observed"
    if completed:
        return "image_item_completed"
    if agent_reply_seen:
        return "agent_reply_observed"
    if require_history and history_args_seen:
        return "history_arguments_seen"
    if failed:
        return "image_item_failed"
    if image_function_call_count:
        return "image_request_seen"
    if turn_status is not None:
        return "turn_nonterminal"
    return "no_events"


def image_last_stage(
    *,
    completed: int,
    failed: int,
    agent_reply_seen: bool,
    history_args_seen: bool,
    require_history: bool,
    image_function_call_count: int,
    turn_status: str | None,
    public_image_item_seen: bool = False,
    snapshot_available: bool = False,
    terminal_snapshot_available: bool = False,
) -> dict[str, object]:
    return {
        "agent_reply_seen": agent_reply_seen,
        "history_arguments_seen": history_args_seen,
        "image_function_call_count": image_function_call_count,
        "image_items_completed": completed,
        "image_items_failed": failed,
        "last_proven_stage": classify_image_stage(
            completed=completed,
            failed=failed,
            agent_reply_seen=agent_reply_seen,
            history_args_seen=history_args_seen,
            require_history=require_history,
            image_function_call_count=image_function_call_count,
            turn_status=turn_status,
        ),
        "public_image_item_seen": public_image_item_seen,
        "require_history": require_history,
        "snapshot_available": snapshot_available,
        "terminal_snapshot_available": terminal_snapshot_available,
        "turn_status": turn_status if turn_status is not None else "missing",
    }


def decoded_image_codec(
    data: bytes,
    timeout: float,
) -> tuple[str, str] | None:
    started = time.monotonic()
    try:
        probe = subprocess.run(
            [
                "ffprobe",
                "-v",
                "error",
                "-protocol_whitelist",
                "pipe",
                "-format_whitelist",
                "apng,jpeg_pipe,png_pipe,webp_pipe",
                "-select_streams",
                "v:0",
                "-show_entries",
                "format=format_name:stream=codec_name,width,height",
                "-of",
                "json",
                "-i",
                "pipe:0",
            ],
            input=data,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            timeout=timeout,
            check=False,
        )
    except OSError as error:
        raise ImageDecoderObservationError from error
    if probe.returncode != 0:
        raise ImageDecoderObservationError
    try:
        payload = json.loads(probe.stdout)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ImageDecoderObservationError from error
    streams = payload.get("streams") if isinstance(payload, dict) else None
    if not isinstance(streams, list) or not streams or not isinstance(streams[0], dict):
        raise ImageDecoderObservationError
    stream = streams[0]
    image_format = payload.get("format") if isinstance(payload, dict) else None
    format_name = image_format.get("format_name") if isinstance(image_format, dict) else None
    codec_name = stream.get("codec_name")
    dimensions = (stream.get("width"), stream.get("height"))
    if (
        not isinstance(codec_name, str)
        or not isinstance(format_name, str)
        or any(
            not isinstance(value, int) or isinstance(value, bool)
            for value in dimensions
        )
    ):
        raise ImageDecoderObservationError
    codec = {
        ("apng", "apng"): ("image/png", ".png"),
        ("mjpeg", "jpeg_pipe"): ("image/jpeg", ".jpg"),
        ("png", "png_pipe"): ("image/png", ".png"),
        ("webp", "webp_pipe"): ("image/webp", ".webp"),
    }.get((codec_name, format_name))
    if codec is None:
        return None
    decode_timeout = timeout - (time.monotonic() - started)
    if decode_timeout <= 0:
        raise subprocess.TimeoutExpired("image decoder", timeout)
    if format_name == "webp_pipe":
        # FFmpeg reports zero dimensions for valid animated WebP. The exact
        # first-frame decoder below owns WebP dimensions instead.
        decode_command = [
            "convert",
            "-quiet",
            "webp:-[0]",
            "-format",
            "%w %h",
            "info:",
        ]
        decode_stdout = subprocess.PIPE
    else:
        if any(value <= 0 for value in dimensions):
            return None
        decode_command = [
            "ffmpeg",
            "-v",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "pipe",
            "-format_whitelist",
            format_name,
            "-f",
            format_name,
            "-i",
            "pipe:0",
            "-map",
            "0:v:0",
            "-frames:v",
            "1",
            "-f",
            "null",
            "-",
        ]
        decode_stdout = subprocess.DEVNULL
    try:
        decode = subprocess.run(
            decode_command,
            input=data,
            stdout=decode_stdout,
            stderr=subprocess.DEVNULL,
            timeout=decode_timeout,
            check=False,
        )
    except OSError as error:
        raise ImageDecoderObservationError from error
    if decode.returncode != 0:
        raise ImageDecoderObservationError
    if format_name == "webp_pipe":
        try:
            decoded_dimensions = tuple(
                int(value) for value in decode.stdout.decode("ascii").split()
            )
        except (AttributeError, UnicodeDecodeError, ValueError) as error:
            raise ImageDecoderObservationError from error
        if len(decoded_dimensions) != 2 or any(
            value <= 0 for value in decoded_dimensions
        ):
            raise ImageDecoderObservationError
    return codec


def wait_for_image_turn(
    server: AppServer,
    deadline: float,
    thread_id: str,
    turn_id: str,
    require_history: bool,
    prior_artifacts: frozenset[Path] = frozenset(),
) -> tuple[dict[str, object], frozenset[Path]]:
    stream_deadline = max(
        time.monotonic(), deadline - TERMINAL_RECONCILIATION_SECONDS
    )
    snapshot_schedule = deque(
        [
            stream_deadline,
            max(
                stream_deadline,
                deadline - (TERMINAL_RECONCILIATION_SECONDS / 2),
            ),
        ]
    )
    raw_arguments_by_call_id: dict[str, dict[str, object]] = {}
    history_qualified_call_ids: set[str] = set()
    agent_reply_seen = False
    failed_image_call_ids: set[str] = set()
    public_image_call_ids: set[str] = set()
    valid_image_evidence_by_call_id: dict[str, tuple[str, str, Path]] = {}
    processed_image_call_ids: set[str] = set()
    turn_status: str | None = None
    terminal_refresh_needed = False
    snapshot_available = False
    terminal_snapshot_available = False
    next_snapshot_request_id = 210 if require_history else 200

    def image_function_call_count() -> int:
        return len(public_image_call_ids & raw_arguments_by_call_id.keys())

    def history_args_seen() -> bool:
        return bool(history_qualified_call_ids & valid_image_evidence_by_call_id.keys())

    def proof_call_ids() -> set[str]:
        valid_call_ids = set(valid_image_evidence_by_call_id)
        if require_history:
            return valid_call_ids & history_qualified_call_ids
        return valid_call_ids

    def proof_ready() -> bool:
        return turn_status == "completed" and agent_reply_seen and bool(proof_call_ids())

    def current_last_stage() -> dict[str, object]:
        last_stage = image_last_stage(
            completed=len(valid_image_evidence_by_call_id),
            failed=len(failed_image_call_ids),
            agent_reply_seen=agent_reply_seen,
            history_args_seen=history_args_seen(),
            require_history=require_history,
            image_function_call_count=image_function_call_count(),
            turn_status=turn_status,
            public_image_item_seen=bool(public_image_call_ids),
            snapshot_available=snapshot_available,
            terminal_snapshot_available=terminal_snapshot_available,
        )
        return last_stage

    def raise_terminal_outcome(cause: BaseException | None = None) -> None:
        last_stage = current_last_stage()
        if turn_status in {"failed", "interrupted"} or (
            turn_status == "completed"
            and terminal_snapshot_available
            and (not agent_reply_seen or not valid_image_evidence_by_call_id)
        ):
            failure: BaseException = LiveScenarioFailed(
                "the Grok image Turn",
                last_stage,
                "semantic_contract",
                "semantic_failure",
            )
        elif turn_status == "completed":
            failure = LiveProofIncomplete("the Grok image Turn", last_stage)
        else:
            failure = LiveDeadlineExpired("the Grok image Turn", last_stage)
        if cause is None:
            raise failure
        raise failure from cause

    def invalid_image_item(payload_stage: str) -> LiveScenarioFailed:
        last_stage = current_last_stage()
        last_stage["last_proven_stage"] = payload_stage
        return LiveScenarioFailed(
            "the Grok image Turn",
            last_stage,
            "semantic_contract",
            "semantic_failure",
        )

    def qualify_history_arguments(call_id: str) -> None:
        arguments = raw_arguments_by_call_id.get(call_id)
        if arguments is None or not require_history:
            return
        recent_images = arguments.get("num_last_images_to_include")
        referenced_images = arguments.get("referenced_image_paths")
        paths_are_empty = referenced_images is None or referenced_images == []
        recent_history = (
            isinstance(recent_images, int)
            and not isinstance(recent_images, bool)
            and recent_images > 0
            and paths_are_empty
        )
        references_prior_artifact = (
            recent_images is None
            and isinstance(referenced_images, list)
            and bool(referenced_images)
            and any(
                isinstance(path, str) and Path(path).resolve() in prior_artifacts
                for path in referenced_images
            )
        )
        if recent_history or references_prior_artifact:
            history_qualified_call_ids.add(call_id)

    def observe_raw_item(item: object) -> None:
        if not isinstance(item, dict) or item.get("type") != "function_call":
            return
        call_id = item.get("call_id")
        arguments = item.get("arguments")
        if not isinstance(call_id, str) or not call_id or not isinstance(arguments, str):
            return
        try:
            parsed_arguments = json.loads(arguments)
        except json.JSONDecodeError:
            return
        if not isinstance(parsed_arguments, dict):
            return
        raw_arguments_by_call_id[call_id] = parsed_arguments
        qualify_history_arguments(call_id)

    def observe_image_item(item: object) -> None:
        if not isinstance(item, dict) or item.get("type") != "imageGeneration":
            return
        call_id = item.get("id")
        if not isinstance(call_id, str) or not call_id:
            return
        public_image_call_ids.add(call_id)
        status = item.get("status")
        if status not in {"completed", "failed"}:
            return
        if call_id in processed_image_call_ids:
            return
        processed_image_call_ids.add(call_id)
        if status == "failed":
            failed_image_call_ids.add(call_id)
            return
        result = item.get("result")
        saved_path = item.get("savedPath")
        if not isinstance(result, str):
            raise invalid_image_item("image_completed_item_seen")
        try:
            decoded = base64.b64decode(result, validate=True)
        except (ValueError, TypeError) as error:
            raise invalid_image_item("image_completed_item_seen") from error
        artifact = Path(saved_path) if isinstance(saved_path, str) else None
        if artifact is None or not artifact.is_file():
            raise invalid_image_item("image_artifact_located")
        if artifact.stat().st_size != len(decoded) or artifact.read_bytes() != decoded:
            raise invalid_image_item("image_artifact_located")
        decoder_last_stage = {
            **current_last_stage(),
            "last_proven_stage": "image_artifact_located",
        }
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise LiveDeadlineExpired(
                "the Grok image decoder",
                decoder_last_stage,
            )
        try:
            codec = decoded_image_codec(decoded, min(10.0, remaining))
        except subprocess.TimeoutExpired as error:
            if time.monotonic() >= deadline:
                raise LiveDeadlineExpired(
                    "the Grok image decoder",
                    decoder_last_stage,
                ) from error
            raise LiveImageDecoderFailed(
                "the Grok image decoder",
                decoder_last_stage,
            ) from error
        except ImageDecoderObservationError as error:
            raise LiveImageDecoderFailed(
                "the Grok image decoder",
                decoder_last_stage,
            ) from error
        if time.monotonic() >= deadline:
            raise LiveDeadlineExpired(
                "the Grok image decoder",
                {
                    **decoder_last_stage,
                    "last_proven_stage": "image_payload_decoded",
                },
            )
        if codec is None:
            raise invalid_image_item("image_payload_decoded")
        image_mime, artifact_extension = codec
        if artifact.suffix.lower() != artifact_extension:
            raise invalid_image_item("image_codec_verified")
        valid_image_evidence_by_call_id[call_id] = (
            image_mime,
            artifact_extension,
            artifact.resolve(),
        )
        qualify_history_arguments(call_id)

    def observe_terminal_turn(turn: object, *, event: bool = False) -> None:
        nonlocal agent_reply_seen, terminal_refresh_needed, turn_status
        if not isinstance(turn, dict) or turn.get("id") != turn_id:
            return
        status = turn.get("status")
        observed_status = status if isinstance(status, str) else None
        if observed_status == "completed":
            turn_status = observed_status
            terminal_reply = terminal_agent_reply(turn, turn_id)
            agent_reply_seen = isinstance(terminal_reply, str) and bool(
                terminal_reply.strip()
            )
            if event:
                terminal_refresh_needed = True
        elif observed_status in {"failed", "interrupted"}:
            turn_status = observed_status
            agent_reply_seen = False
        elif turn_status is None:
            turn_status = observed_status
        items = turn.get("items")
        if isinstance(items, list):
            for item in items:
                observe_image_item(item)

    def reconcile(read_deadline: float) -> None:
        nonlocal next_snapshot_request_id, snapshot_available
        nonlocal terminal_snapshot_available
        request_id = next_snapshot_request_id
        next_snapshot_request_id += 2
        try:
            thread = read_thread_snapshot(
                server,
                request_id,
                thread_id,
                read_deadline,
            )
        except LiveDeadlineExpired:
            raise
        except SystemExit as error:
            raise LiveObservationFailed(
                "the Grok image Thread snapshot",
                current_last_stage(),
            ) from error
        if thread is None:
            return
        turn = snapshot_turn(thread, turn_id)
        if turn is None:
            return
        snapshot_available = True
        terminal_snapshot_available = (
            terminal_snapshot_available or turn.get("status") == "completed"
        )
        observe_terminal_turn(turn)

    while time.monotonic() < deadline:
        if proof_ready():
            break
        if terminal_refresh_needed:
            terminal_refresh_needed = False
            try:
                reconcile(snapshot_schedule[0] if snapshot_schedule else deadline)
            except LiveDeadlineExpired:
                pass
            if proof_ready():
                break
        while (
            snapshot_schedule
            and time.monotonic() >= snapshot_schedule[0]
            and time.monotonic() < deadline
        ):
            snapshot_schedule.popleft()
            try:
                reconcile(snapshot_schedule[0] if snapshot_schedule else deadline)
            except LiveDeadlineExpired:
                pass
            if proof_ready():
                break
        if proof_ready():
            break
        message_deadline = snapshot_schedule[0] if snapshot_schedule else deadline
        try:
            message = server.next_message(message_deadline, "the Grok image Turn")
        except LiveDeadlineExpired as error:
            if snapshot_schedule and time.monotonic() < deadline:
                snapshot_schedule.popleft()
                try:
                    reconcile(snapshot_schedule[0] if snapshot_schedule else deadline)
                except LiveDeadlineExpired:
                    pass
                if proof_ready():
                    break
                continue
            raise_terminal_outcome(error)
        params = message.get("params")
        if not isinstance(params, dict) or params.get("threadId") != thread_id:
            continue
        method = message.get("method")
        if method in {"rawResponseItem/completed", "item/completed"} and (
            params.get("turnId") != turn_id
        ):
            continue
        if method == "rawResponseItem/completed":
            observe_raw_item(params.get("item"))
        elif method == "item/completed":
            item = params.get("item")
            observe_image_item(item)
        elif method == "turn/completed":
            turn = params.get("turn")
            if not isinstance(turn, dict) or turn.get("id") != turn_id:
                continue
            observe_terminal_turn(turn, event=True)
            if turn_status != "completed":
                raise LiveScenarioFailed(
                    "the Grok image Turn",
                    current_last_stage(),
                    "semantic_contract",
                    "semantic_failure",
                )

    if proof_ready():
        selected_call_id = sorted(proof_call_ids())[0]
        image_mime, artifact_extension, artifact = valid_image_evidence_by_call_id[
            selected_call_id
        ]
        return ({
            "agent_reply_seen": True,
            "artifact_match": True,
            "image_decodable": True,
            "history_arguments_verified": history_args_seen(),
            "image_items_failed": len(failed_image_call_ids),
            "image_items_completed": len(valid_image_evidence_by_call_id),
            "image_mime": image_mime,
            "artifact_extension": artifact_extension,
            "status": "completed",
        }, frozenset({artifact for _, _, artifact in valid_image_evidence_by_call_id.values()}))

    raise_terminal_outcome()


def turn_input(prompt: str) -> list[dict[str, object]]:
    return [{"text": prompt, "textElements": [], "type": "text"}]


def run_smoke(
    archive: Path,
    config: Path,
    evidence_path: Path,
    source_sha: str,
    validator_sha: str,
    run_id: str,
    scenario: str,
    mode: str = RELEASE_MODE,
) -> None:
    """Run one scenario and write secret-safe evidence.

    In release mode a failed scenario writes last-stage evidence and re-raises
    so the calling gate fails. In observation mode the same evidence is written
    with ``mode: observation`` and the failure is swallowed; observation
    evidence never satisfies a release contract.
    """
    if mode not in MODES:
        raise SystemExit(f"unknown live mode: {mode}")
    with tempfile.TemporaryDirectory() as temporary:
        temporary_path = Path(temporary)
        root = extract_archive(archive, temporary_path / "artifact")
        codex_home = temporary_path / "home"
        workspace = temporary_path / "workspace"
        codex_home.mkdir()
        workspace.mkdir()
        shutil.copy2(config, codex_home / "config.toml")

        with config.open("rb") as handle:
            config_data = tomllib.load(handle)
        providers = config_data.get("model_providers")
        provider = providers.get("grok") if isinstance(providers, dict) else None
        token = provider.get("experimental_bearer_token") if isinstance(provider, dict) else None
        if not isinstance(token, str) or not token:
            raise SystemExit("secret profile has no Grok bearer token")

        server = AppServer(
            root / "bin/grokex-bin",
            codex_home,
            workspace,
            token,
        )
        runner_turn_submission_count = 0
        thread_model: str | None = None
        stages = StageClock()
        identity = {
            "archive": archive.name,
            "archive_sha256": sha256(archive),
            "catalog": "release-bundled",
            "contract_sha256": sha256(LIVE_CONTRACTS_PATH),
            "mode": mode,
            "provider": "grok",
            "scenario": scenario,
            "source_sha": source_sha,
            "story": STORY_BY_SCENARIO[scenario],
            "validation_run": run_id,
            "validator_sha": validator_sha,
            **(
                {"multi_agent_version": "v2", "reasoning_effort": "ultra"}
                if scenario == COLLABORATION_SCENARIO
                else {}
            ),
        }
        try:
            server.request(
                1,
                "initialize",
                {
                    "clientInfo": {"name": "grokex-release", "version": VERSION},
                    "capabilities": {"experimentalApi": True},
                },
            )
            server.send({"method": "initialized"})
            stages.mark("initialized")

            models_response = server.request(
                2,
                "model/list",
                {"cursor": None, "includeHidden": None, "limit": 100},
            )
            models = models_response.get("data")
            if not isinstance(models, list):
                raise SystemExit("model/list did not return a catalog")
            matching = [model for model in models if isinstance(model, dict) and model.get("id") == "grok-4.6"]
            if len(matching) != 1:
                raise SystemExit("release catalog does not contain exact grok-4.6")
            model = matching[0]
            if scenario == COLLABORATION_SCENARIO:
                efforts = {
                    option.get("reasoningEffort")
                    for option in model.get("supportedReasoningEfforts", [])
                    if isinstance(option, dict)
                }
                if model.get("multiAgentVersion") != "v2" or "ultra" not in efforts:
                    raise SystemExit("grok-4.6 collaboration metadata is incomplete")
            stages.mark("catalog_verified")

            thread_params: dict[str, object] = {
                "cwd": str(workspace),
                "model": "grok-4.6",
                "modelProvider": "grok",
            }
            if scenario == COLLABORATION_SCENARIO:
                thread_params["experimentalRawEvents"] = True
            if scenario == CONTINUATION_SCENARIO:
                thread_params["experimentalRawEvents"] = True
                thread_params["dynamicTools"] = [
                    {
                        "name": TOOL_NAME,
                        "description": "Return the fixed live validation marker.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {},
                            "additionalProperties": False,
                        },
                    }
                ]
            elif scenario == IMAGE_SCENARIO:
                thread_params["experimentalRawEvents"] = True
            thread_response = server.request(3, "thread/start", thread_params)
            thread = thread_response.get("thread")
            thread_model = thread_response.get("model")
            if (
                not isinstance(thread, dict)
                or thread_response.get("modelProvider") != "grok"
                or thread_model != "grok-4.6"
            ):
                raise SystemExit("thread/start did not bind grok/grok-4.6")
            thread_id = thread.get("id")
            if not isinstance(thread_id, str) or not thread_id:
                raise SystemExit("thread/start returned no thread identity")
            stages.mark("thread_started")

            if scenario == BASIC_SCENARIO:
                prompt = (
                    f"Reply with exactly {BASIC_EXPECTED_AGENT_REPLY} and no other text."
                )
                runner_turn_submission_count = 1
                stages.mark("turn_submission_attempted")
                stages.start_turn()
                turn_response = server.request(
                    4,
                    "turn/start",
                    {"input": turn_input(prompt), "threadId": thread_id},
                )
                stages.mark("turn_start_response_received")
                turn_id = require_turn_start_identity(
                    turn_response, "the single basic Grok Turn"
                )
                stages.mark("turn_submitted")
                turn_evidence = wait_for_basic_turn(
                    server,
                    time.monotonic() + BASIC_TURN_SECONDS,
                    thread_id,
                    turn_id,
                )
                stages.finish_turn()
            elif scenario == CONTINUATION_SCENARIO:
                prompt = (
                    f"Use the {TOOL_NAME} result, then reply "
                    f"with exactly {EXPECTED_AGENT_REPLY} and no other text."
                )
                runner_turn_submission_count = 1
                stages.mark("turn_submission_attempted")
                stages.start_turn()
                turn_response = server.request(
                    4,
                    "turn/start",
                    {"input": turn_input(prompt), "threadId": thread_id},
                )
                stages.mark("turn_start_response_received")
                turn_id = require_turn_start_identity(
                    turn_response, "the Grok tool-continuation Turn"
                )
                stages.mark("turn_submitted")
                turn_evidence = wait_for_verified_turn(
                    server,
                    time.monotonic() + CONTINUATION_TURN_SECONDS,
                    thread_id,
                    turn_id,
                )
                stages.finish_turn()
                stages.mark("first_turn_completed")

                history_prompt = (
                    "Reply with exactly the result returned by grokex_live_probe in the "
                    "previous Turn and no other text. Do not call any tool."
                )
                runner_turn_submission_count = 2
                stages.mark("history_turn_submission_attempted")
                stages.start_turn()
                history_turn_response = server.request(
                    5,
                    "turn/start",
                    {"input": turn_input(history_prompt), "threadId": thread_id},
                )
                stages.mark("history_turn_start_response_received")
                history_turn_id = require_turn_start_identity(
                    history_turn_response,
                    "the Grok history-replay Turn",
                    frozenset({turn_id}),
                )
                stages.mark("history_turn_submitted")
                history_evidence = wait_for_terminal_reply(
                    server,
                    time.monotonic() + CONTINUATION_TURN_SECONDS,
                    thread_id,
                    history_turn_id,
                    HISTORY_EXPECTED_AGENT_REPLY,
                    "the Grok history-replay Turn",
                )
                stages.finish_turn()
                turn_evidence = {
                    **turn_evidence,
                    "history_response_assertion": history_evidence[
                        "response_assertion"
                    ],
                    "same_thread_history": "completed",
                }
            elif scenario == COLLABORATION_SCENARIO:
                prompt = (
                    "Delegate one bounded task to a child named live_child using the default "
                    "full-history fork. Tell the child: Generate a fresh UUID v4 and reply with "
                    "exactly its canonical lowercase text and no other text. Wait for that child "
                    "to complete, then reply with exactly the UUID returned by the child and no "
                    "other text."
                )
                runner_turn_submission_count = 1
                stages.mark("turn_submission_attempted")
                stages.start_turn()
                turn_response = server.request(
                    4,
                    "turn/start",
                    {
                        "effort": "ultra",
                        "input": turn_input(prompt),
                        "threadId": thread_id,
                    },
                )
                stages.mark("turn_start_response_received")
                turn_id = require_turn_start_identity(
                    turn_response, "the Grok Ultra collaboration Turn"
                )
                stages.mark("turn_submitted")
                turn_evidence, _ = wait_for_collaboration_turn(
                    server,
                    time.monotonic() + COLLABORATION_TURN_SECONDS,
                    thread_id,
                    turn_id,
                )
                stages.finish_turn()
                stages.mark("collaboration_completed")
                stages.mark("child_binding_verified")
            else:
                generation_artifacts: frozenset[Path] = frozenset()
                image_turn_ids: set[str] = set()
                image_evidence_by_phase: dict[str, dict[str, object]] = {}
                for phase, request_id, prompt, require_history in [
                    (
                        "generation",
                        4,
                        "Generate an image of a blue circle on a plain white background.",
                        False,
                    ),
                    (
                        "edit",
                        5,
                        "Edit the image you just generated so the circle is green while "
                        "keeping the plain white background.",
                        True,
                    ),
                ]:
                    runner_turn_submission_count += 1
                    stage_prefix = "history_turn" if require_history else "turn"
                    stages.mark(f"{stage_prefix}_submission_attempted")
                    stages.start_turn()
                    turn_response = server.request(
                        request_id,
                        "turn/start",
                        {"input": turn_input(prompt), "threadId": thread_id},
                    )
                    stages.mark(f"{stage_prefix}_start_response_received")
                    turn_id = require_turn_start_identity(
                        turn_response,
                        "the Grok image Turn",
                        frozenset(image_turn_ids),
                    )
                    image_turn_ids.add(turn_id)
                    stages.mark(f"{stage_prefix}_submitted")
                    image_evidence, artifacts = wait_for_image_turn(
                        server,
                        time.monotonic() + IMAGE_TURN_SECONDS,
                        thread_id,
                        turn_id,
                        require_history,
                        generation_artifacts,
                    )
                    stages.finish_turn()
                    image_evidence_by_phase[phase] = image_evidence
                    if phase == "generation":
                        generation_artifacts = artifacts
                generation_evidence = image_evidence_by_phase["generation"]
                edit_evidence = image_evidence_by_phase["edit"]
                turn_evidence = {
                    "edit_agent_reply_seen": edit_evidence["agent_reply_seen"],
                    "edit_artifact_extension": edit_evidence["artifact_extension"],
                    "edit_artifact_match": edit_evidence["artifact_match"],
                    "edit_completion": edit_evidence["status"],
                    "edit_image_decodable": edit_evidence["image_decodable"],
                    "edit_image_mime": edit_evidence["image_mime"],
                    "generation_agent_reply_seen": generation_evidence[
                        "agent_reply_seen"
                    ],
                    "generation_artifact_extension": generation_evidence[
                        "artifact_extension"
                    ],
                    "generation_artifact_match": generation_evidence[
                        "artifact_match"
                    ],
                    "generation_completion": generation_evidence["status"],
                    "generation_image_decodable": generation_evidence[
                        "image_decodable"
                    ],
                    "generation_image_mime": generation_evidence["image_mime"],
                    "history_arguments_verified": edit_evidence[
                        "history_arguments_verified"
                    ],
                    "image_items_completed": (
                        generation_evidence["image_items_completed"]
                        + edit_evidence["image_items_completed"]
                    ),
                    "image_items_failed": (
                        generation_evidence["image_items_failed"]
                        + edit_evidence["image_items_failed"]
                    ),
                    "same_thread": True,
                    "status": "completed",
                }

            evidence = {
                **identity,
                "model": thread_model,
                "runner_turn_submission_count": runner_turn_submission_count,
                **turn_evidence,
                **stages.evidence(),
            }
            evidence_path.write_text(
                json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8"
            )
        except SystemExit as error:
            if isinstance(error, LiveScenarioFailed):
                failure_evidence = dict(error.last_stage)
                failure_evidence.setdefault("last_proven_stage", stages.current)
            else:
                failure_evidence = {
                    "does_not_prove": "product_root_cause",
                    "failure_category": "app_server_or_semantic_contract",
                    "last_proven_stage": stages.current,
                    "outcome": "semantic_failure",
                }
            evidence = {
                **identity,
                "model": thread_model,
                "runner_turn_submission_count": runner_turn_submission_count,
                **failure_evidence,
                **stages.evidence(),
            }
            evidence_path.write_text(
                json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8"
            )
            if mode == OBSERVATION_MODE and isinstance(error, LiveScenarioFailed):
                return
            raise
        finally:
            server.close()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--validator-sha", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--scenario", choices=SCENARIOS, required=True)
    parser.add_argument("--mode", choices=MODES, default=RELEASE_MODE)
    args = parser.parse_args()
    oracle = LIVE_CONTRACTS["scenarios"][args.scenario].get("oracle", STREAM_ORACLE)
    if oracle != STREAM_ORACLE:
        raise SystemExit(
            f"scenario {args.scenario} is owned by oracle {oracle} (grokex/validator), not {STREAM_ORACLE}"
        )
    run_smoke(
        args.archive,
        args.config,
        args.evidence,
        args.source_sha,
        args.validator_sha,
        args.run_id,
        args.scenario,
        args.mode,
    )


if __name__ == "__main__":
    main()
