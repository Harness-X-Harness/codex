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


TAG = "grokex-v0.151.0"
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
    BASIC_SCENARIO: "grokex-provider-profile-startup",
    CONTINUATION_SCENARIO: "grokex-encrypted-reasoning-history-continuation",
    COLLABORATION_SCENARIO: "grokex-provider-binding-lifecycle",
    IMAGE_SCENARIO: "grokex-image-generation-history-edit",
}
BASIC_EXPECTED_AGENT_REPLY = "GROKEX_BASIC_RESPONSE_OK"
TOOL_NAME = "grokex_live_probe"
TOOL_OUTPUT_MARKER = "GROKEX_LIVE_TOOL_OK"
EXPECTED_AGENT_REPLY = "GROKEX_LIVE_RESPONSE_OK"
HISTORY_EXPECTED_AGENT_REPLY = TOOL_OUTPUT_MARKER
BASIC_TURN_SECONDS = 120
CONTINUATION_TURN_SECONDS = 120
COLLABORATION_TURN_SECONDS = 360
IMAGE_TURN_SECONDS = 180


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
    root = destination / TAG
    if not root.is_dir():
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

    def request(self, request_id: int, method: str, params: dict[str, object]) -> dict[str, object]:
        self.send({"id": request_id, "method": method, "params": params})
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            message = self._next_incoming_message(deadline, method)
            if message.get("id") != request_id or "method" in message:
                self.deferred_messages.append(message)
                continue
            if "error" in message:
                raise SystemExit(f"App Server rejected {method}")
            response = message.get("response", message.get("result"))
            if not isinstance(response, dict):
                raise SystemExit(f"App Server returned an invalid {method} response")
            return response
        raise SystemExit(f"App Server timed out during {method}")

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
        if isinstance(item, dict) and item.get("type") == "agentMessage":
            return item.get("text")
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


def is_default_full_history_spawn(parsed_arguments: dict[str, object]) -> bool:
    fork_turns = parsed_arguments.get("fork_turns")
    if fork_turns is None:
        return True
    if not isinstance(fork_turns, str):
        return False
    fork_turns = fork_turns.strip()
    return fork_turns == "" or fork_turns.casefold() == "all"


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
    spawn_started_count: int,
    provider_response_count: int,
) -> str:
    if (
        root_status == "completed"
        and completed_default_child_count
        and parent_reply_seen
        and default_spawn_count
        and default_child_count
    ):
        return "completed"
    if parent_reply_seen:
        return "parent_reply_seen"
    if root_status == "completed":
        return "parent_turn_completed"
    if completed_default_child_count:
        return "child_completed"
    if default_child_count:
        return "child_created"
    if spawn_started_count:
        return "spawn_started"
    if default_spawn_count:
        return "default_spawn_requested"
    if provider_response_count:
        return "provider_response_seen"
    return "no_events"


def collaboration_last_stage(
    *,
    root_status: str | None,
    child_thread_ids: set[str],
    default_child_thread_ids: set[str],
    child_completed_thread_ids: set[str],
    child_activity_completed_thread_ids: set[str],
    child_reply_thread_ids: set[str],
    default_spawn_call_ids: set[str],
    explicit_fork_spawn_count: int,
    failed_tool_count: int,
    missing_spawn_identity_count: int,
    parent_reply_seen: bool,
    response_counts: dict[str, int],
    spawn_started_count: int,
    wait_completed_count: int,
    unexpected_tool_count: int,
) -> dict[str, object]:
    completed_default_child_count = len(
        default_child_thread_ids
        & child_completed_thread_ids
        & child_activity_completed_thread_ids
        & child_reply_thread_ids
    )
    return {
        "child_activity_completed_count": len(child_activity_completed_thread_ids),
        "child_completed_count": len(child_completed_thread_ids),
        "child_count": len(child_thread_ids),
        "child_reply_count": len(child_reply_thread_ids),
        "completed_default_child_count": completed_default_child_count,
        "default_child_count": len(default_child_thread_ids),
        "default_spawn_count": len(default_spawn_call_ids),
        "explicit_fork_spawn_count": explicit_fork_spawn_count,
        "failed_collaboration_tool_count": failed_tool_count,
        "last_proven_stage": classify_collaboration_stage(
            root_status=root_status,
            parent_reply_seen=parent_reply_seen,
            default_spawn_count=len(default_spawn_call_ids),
            default_child_count=len(default_child_thread_ids),
            completed_default_child_count=completed_default_child_count,
            spawn_started_count=spawn_started_count,
            provider_response_count=sum(response_counts.values()),
        ),
        "missing_spawn_identity_count": missing_spawn_identity_count,
        "parent_reply_seen": parent_reply_seen,
        "provider_response_count": sum(response_counts.values()),
        "root_status": root_status,
        "spawn_count": spawn_started_count,
        "unexpected_collaboration_tool_count": unexpected_tool_count,
        "wait_count": wait_completed_count,
    }


def wait_for_collaboration_turn(
    server: AppServer,
    deadline: float,
    root_thread_id: str,
    root_turn_id: str,
) -> tuple[dict[str, object], frozenset[str]]:
    child_activity_completed_thread_ids: set[str] = set()
    child_started_turns: set[tuple[str, str]] = set()
    child_terminal_turns: dict[tuple[str, str], tuple[object, object]] = {}
    child_thread_ids: set[str] = set()
    default_child_thread_ids: set[str] = set()
    default_spawn_call_ids: set[str] = set()
    started_children_by_call_id: dict[str, str] = {}
    explicit_fork_spawn_count = 0
    missing_spawn_identity_count = 0
    response_counts: dict[str, int] = {}
    root_status = None
    root_terminal_reply = None
    spawn_started_count = 0
    wait_completed_count = 0
    failed_tool_count = 0
    unexpected_tool_count = 0

    def child_terminal_state() -> tuple[set[str], set[str]]:
        completed: set[str] = set()
        replied: set[str] = set()
        for child_turn, (status, reply) in child_terminal_turns.items():
            if child_turn not in child_started_turns:
                continue
            child_thread_id, _ = child_turn
            if status == "completed":
                completed.add(child_thread_id)
            if status == "completed" and is_canonical_uuid_v4(reply):
                replied.add(child_thread_id)
        return completed, replied

    def matching_child_thread_ids() -> set[str]:
        if not is_canonical_uuid_v4(root_terminal_reply):
            return set()
        matching: set[str] = set()
        for child_turn, (status, reply) in child_terminal_turns.items():
            child_thread_id, _ = child_turn
            if (
                child_turn in child_started_turns
                and child_thread_id in default_child_thread_ids
                and child_thread_id in child_activity_completed_thread_ids
                and status == "completed"
                and reply == root_terminal_reply
            ):
                matching.add(child_thread_id)
        return matching

    def current_semantics() -> tuple[set[str], set[str], bool]:
        child_completed_thread_ids, child_reply_thread_ids = child_terminal_state()
        parent_reply_seen = bool(matching_child_thread_ids())
        return child_completed_thread_ids, child_reply_thread_ids, parent_reply_seen

    def current_last_stage() -> dict[str, object]:
        child_completed_thread_ids, child_reply_thread_ids, parent_reply_seen = (
            current_semantics()
        )
        return collaboration_last_stage(
            root_status=root_status,
            child_thread_ids=child_thread_ids,
            default_child_thread_ids=default_child_thread_ids,
            child_completed_thread_ids=child_completed_thread_ids,
            child_activity_completed_thread_ids=child_activity_completed_thread_ids,
            child_reply_thread_ids=child_reply_thread_ids,
            default_spawn_call_ids=default_spawn_call_ids,
            explicit_fork_spawn_count=explicit_fork_spawn_count,
            failed_tool_count=failed_tool_count,
            missing_spawn_identity_count=missing_spawn_identity_count,
            parent_reply_seen=parent_reply_seen,
            response_counts=response_counts,
            spawn_started_count=spawn_started_count,
            wait_completed_count=wait_completed_count,
            unexpected_tool_count=unexpected_tool_count,
        )

    while time.monotonic() < deadline:
        child_completed_thread_ids, child_reply_thread_ids, parent_reply_seen = (
            current_semantics()
        )
        if root_status == "completed" and parent_reply_seen:
            break
        try:
            message = server.next_message(deadline, "the Grok Ultra collaboration Turn")
        except LiveDeadlineExpired as error:
            raise LiveDeadlineExpired(
                "the Grok Ultra collaboration Turn",
                current_last_stage(),
            ) from error
        method = message.get("method")
        params = message.get("params")
        if not isinstance(params, dict):
            continue
        thread_id = params.get("threadId")

        if method == "rawResponse/completed" and isinstance(thread_id, str):
            response_counts[thread_id] = response_counts.get(thread_id, 0) + 1
            continue

        if method == "turn/started" and thread_id != root_thread_id:
            turn = params.get("turn")
            turn_id = turn.get("id") if isinstance(turn, dict) else None
            if isinstance(thread_id, str) and isinstance(turn_id, str) and turn_id:
                child_started_turns.add((thread_id, turn_id))
            continue

        if (
            method == "rawResponseItem/completed"
            and thread_id == root_thread_id
            and params.get("turnId") == root_turn_id
        ):
            item = params.get("item")
            if (
                not isinstance(item, dict)
                or item.get("type") != "function_call"
                or item.get("namespace") != "collaboration"
                or item.get("name") != "spawn_agent"
            ):
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
            if is_default_full_history_spawn(parsed_arguments):
                default_spawn_call_ids.add(call_id)
                child_thread_id = started_children_by_call_id.get(call_id)
                if child_thread_id is not None:
                    default_child_thread_ids.add(child_thread_id)
            else:
                explicit_fork_spawn_count += 1
            continue

        if (
            method == "item/completed"
            and thread_id == root_thread_id
            and params.get("turnId") == root_turn_id
        ):
            item = params.get("item")
            if not isinstance(item, dict):
                continue
            if item.get("type") == "subAgentActivity":
                agent_thread_id = item.get("agentThreadId")
                kind = item.get("kind")
                if isinstance(agent_thread_id, str) and agent_thread_id:
                    child_thread_ids.add(agent_thread_id)
                    if kind == "started":
                        spawn_started_count += 1
                        activity_id = item.get("id")
                        if isinstance(activity_id, str) and activity_id:
                            started_children_by_call_id[activity_id] = agent_thread_id
                            if activity_id in default_spawn_call_ids:
                                default_child_thread_ids.add(agent_thread_id)
                    elif kind == "completed":
                        child_activity_completed_thread_ids.add(agent_thread_id)
                continue
            if item.get("type") == "collabAgentToolCall":
                tool = item.get("tool")
                if item.get("status") != "completed":
                    failed_tool_count += 1
                    continue
                if tool == "spawnAgent":
                    receiver_thread_ids = item.get("receiverThreadIds")
                    if not isinstance(receiver_thread_ids, list):
                        receiver_thread_ids = []
                    child_thread_ids.update(
                        receiver_thread_id
                        for receiver_thread_id in receiver_thread_ids
                        if isinstance(receiver_thread_id, str) and receiver_thread_id
                    )
                elif tool == "wait":
                    wait_completed_count += 1
                else:
                    unexpected_tool_count += 1
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
                root_status = status
                root_terminal_reply = terminal_agent_reply(turn, root_turn_id)
                if root_status != "completed" or not is_canonical_uuid_v4(
                    root_terminal_reply
                ):
                    raise LiveScenarioFailed(
                        "the Grok Ultra collaboration Turn",
                        current_last_stage(),
                        "semantic_contract",
                        "semantic_failure",
                    )
            elif isinstance(thread_id, str) and isinstance(event_turn_id, str):
                child_terminal_turns[(thread_id, event_turn_id)] = (
                    status,
                    terminal_agent_reply(turn, event_turn_id),
                )

    child_completed_thread_ids, child_reply_thread_ids, parent_reply_seen = (
        current_semantics()
    )
    last_stage = current_last_stage()
    if last_stage["last_proven_stage"] != "completed":
        raise LiveDeadlineExpired("the Grok Ultra collaboration Turn", last_stage)
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
    if not parent_reply_seen:
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
        "child_count": len(child_thread_ids),
        "child_response_assertion": "canonical_uuid_v4",
        "default_full_history": "completed",
        "explicit_fork_spawn_count": explicit_fork_spawn_count,
        "failed_collaboration_tool_count": failed_tool_count,
        "missing_spawn_identity_count": missing_spawn_identity_count,
        "parent_completion": "completed",
        "provider_response_count": provider_response_count,
        "response_assertion": "child_echo_match",
        "spawn_count": spawn_started_count,
        "status": root_status,
        "unexpected_collaboration_tool_count": unexpected_tool_count,
        "wait_count": wait_completed_count,
        "result_delivery": "completed",
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
    if turn_status is not None:
        return "turn_completed"
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
        "require_history": require_history,
        "turn_status": turn_status if turn_status is not None else "missing",
    }


def supported_image_codec(data: bytes) -> tuple[str, str] | None:
    if data.startswith(b"\xff\xd8\xff") and data.endswith(b"\xff\xd9"):
        return ("image/jpeg", ".jpg")
    if data.startswith(b"\x89PNG\r\n\x1a\n") and data.endswith(
        b"\x00\x00\x00\x00IEND\xaeB\x60\x82"
    ):
        return ("image/png", ".png")
    if (
        len(data) >= 12
        and data.startswith(b"RIFF")
        and data[8:12] == b"WEBP"
        and int.from_bytes(data[4:8], "little") + 8 == len(data)
    ):
        return ("image/webp", ".webp")
    return None


def wait_for_image_turn(
    server: AppServer,
    deadline: float,
    thread_id: str,
    turn_id: str,
    require_history: bool,
    prior_artifacts: frozenset[Path] = frozenset(),
) -> tuple[dict[str, object], frozenset[Path]]:
    completed = 0
    failed = 0
    history_args_seen = False
    image_candidate_call_ids: set[str] = set()
    history_qualified_call_ids: set[str] = set()
    agent_reply_seen = False
    image_function_call_count = 0
    image_mime: str | None = None
    artifact_extension: str | None = None
    artifacts: set[Path] = set()
    image_items_by_call_id: dict[str, dict[str, object]] = {}
    processed_image_call_ids: set[str] = set()
    turn_status: str | None = None

    def invalid_image_item(payload_stage: str) -> LiveScenarioFailed:
        last_stage = image_last_stage(
            completed=completed,
            failed=failed,
            agent_reply_seen=agent_reply_seen,
            history_args_seen=history_args_seen,
            require_history=require_history,
            image_function_call_count=image_function_call_count,
            turn_status=turn_status,
        )
        last_stage["last_proven_stage"] = payload_stage
        return LiveScenarioFailed(
            "the Grok image Turn",
            last_stage,
            "semantic_contract",
            "semantic_failure",
        )

    def process_correlated_image_item(call_id: str) -> None:
        nonlocal artifact_extension, completed, failed, image_mime
        if call_id in processed_image_call_ids:
            return
        item = image_items_by_call_id.get(call_id)
        if item is None or call_id not in image_candidate_call_ids:
            return
        if require_history and call_id not in history_qualified_call_ids:
            return
        processed_image_call_ids.add(call_id)
        if item.get("status") != "completed":
            failed += 1
            return
        result = item.get("result")
        saved_path = item.get("savedPath")
        if not isinstance(result, str):
            raise invalid_image_item("image_completed_item_seen")
        try:
            decoded = base64.b64decode(result, validate=True)
        except (ValueError, TypeError) as error:
            raise invalid_image_item("image_completed_item_seen") from error
        codec = supported_image_codec(decoded)
        if codec is None:
            raise invalid_image_item("image_payload_decoded")
        image_mime, artifact_extension = codec
        artifact = Path(saved_path) if isinstance(saved_path, str) else None
        if (
            artifact is None
            or artifact.suffix.lower() != artifact_extension
            or not artifact.is_file()
        ):
            raise invalid_image_item("image_codec_verified")
        if artifact.stat().st_size != len(decoded) or artifact.read_bytes() != decoded:
            raise invalid_image_item("image_artifact_located")
        artifacts.add(artifact.resolve())
        completed += 1

    while time.monotonic() < deadline:
        try:
            message = server.next_message(deadline, "the Grok image Turn")
        except LiveDeadlineExpired as error:
            raise LiveDeadlineExpired(
                "the Grok image Turn",
                image_last_stage(
                    completed=completed,
                    failed=failed,
                    agent_reply_seen=agent_reply_seen,
                    history_args_seen=history_args_seen,
                    require_history=require_history,
                    image_function_call_count=image_function_call_count,
                    turn_status=turn_status,
                ),
            ) from error
        params = message.get("params")
        if not isinstance(params, dict) or params.get("threadId") != thread_id:
            continue
        method = message.get("method")
        if method in {"rawResponseItem/completed", "item/completed"} and (
            params.get("turnId") != turn_id
        ):
            continue
        if method == "rawResponseItem/completed":
            item = params.get("item")
            if (
                isinstance(item, dict)
                and item.get("type") == "function_call"
                and item.get("namespace") == "image_gen"
                and item.get("name") == "imagegen"
            ):
                try:
                    arguments = json.loads(item.get("arguments", ""))
                except (TypeError, json.JSONDecodeError):
                    arguments = None
                call_id = item.get("call_id")
                if (
                    not isinstance(arguments, dict)
                    or not isinstance(arguments.get("prompt"), str)
                    or not isinstance(call_id, str)
                    or not call_id
                ):
                    continue
                image_function_call_count += 1
                recent_images = arguments.get("num_last_images_to_include")
                referenced_images = arguments.get("referenced_image_paths")
                paths_are_empty = referenced_images is None or referenced_images == []
                references_prior_artifacts = (
                    recent_images is None
                    and isinstance(referenced_images, list)
                    and bool(referenced_images)
                    and all(
                        isinstance(path, str)
                        and Path(path).resolve() in prior_artifacts
                        for path in referenced_images
                    )
                )
                recent_history = (
                    isinstance(recent_images, int)
                    and not isinstance(recent_images, bool)
                    and recent_images > 0
                    and paths_are_empty
                )
                generation = recent_images is None and paths_are_empty
                if require_history and (recent_history or references_prior_artifacts):
                    image_candidate_call_ids.add(call_id)
                    history_qualified_call_ids.add(call_id)
                    history_args_seen = True
                elif not require_history and generation:
                    image_candidate_call_ids.add(call_id)
                process_correlated_image_item(call_id)
        elif method == "item/completed":
            item = params.get("item")
            if isinstance(item, dict) and item.get("type") == "agentMessage":
                text = item.get("text")
                agent_reply_seen = isinstance(text, str) and bool(text.strip())
            elif isinstance(item, dict) and item.get("type") == "imageGeneration":
                item_id = item.get("id")
                if isinstance(item_id, str) and item_id:
                    image_items_by_call_id[item_id] = item
                    process_correlated_image_item(item_id)
        elif method == "turn/completed":
            turn = params.get("turn")
            if not isinstance(turn, dict) or turn.get("id") != turn_id:
                continue
            turn_status = turn.get("status")
            terminal_items = turn.get("items")
            agent_reply_seen = isinstance(terminal_items, list) and any(
                isinstance(item, dict)
                and item.get("type") == "agentMessage"
                and isinstance(item.get("text"), str)
                and bool(item["text"].strip())
                for item in terminal_items
            )
            if (
                turn_status != "completed"
                or completed < 1
                or not agent_reply_seen
                or (require_history and not history_args_seen)
            ):
                raise LiveScenarioFailed(
                    "the Grok image Turn",
                    image_last_stage(
                        completed=completed,
                        failed=failed,
                        agent_reply_seen=agent_reply_seen,
                        history_args_seen=history_args_seen,
                        require_history=require_history,
                        image_function_call_count=image_function_call_count,
                        turn_status=turn_status,
                    ),
                    "semantic_contract",
                    "semantic_failure",
                )
            return ({
                "agent_reply_seen": True,
                "artifact_match": True,
                "history_arguments_verified": history_args_seen,
                "image_items_failed": failed,
                "image_items_completed": completed,
                "image_mime": image_mime,
                "artifact_extension": artifact_extension,
                "status": "completed",
            }, frozenset(artifacts))
    raise LiveDeadlineExpired(
        "the Grok image Turn",
        image_last_stage(
            completed=completed,
            failed=failed,
            agent_reply_seen=agent_reply_seen,
            history_args_seen=history_args_seen,
            require_history=require_history,
            image_function_call_count=image_function_call_count,
            turn_status=turn_status,
        ),
    )


def run_smoke(
    archive: Path,
    config: Path,
    evidence_path: Path,
    source_sha: str,
    validator_sha: str,
    run_id: str,
    scenario: str,
) -> None:
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
        lifecycle_stage = "app_server_started"
        try:
            server.request(
                1,
                "initialize",
                {
                    "clientInfo": {"name": "grokex-release", "version": "0.151.0"},
                    "capabilities": {"experimentalApi": True},
                },
            )
            server.send({"method": "initialized"})
            lifecycle_stage = "initialized"

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
            lifecycle_stage = "catalog_verified"

            thread_params: dict[str, object] = {
                "cwd": str(workspace),
                "ephemeral": True,
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
            lifecycle_stage = "thread_started"

            if scenario == BASIC_SCENARIO:
                prompt = (
                    f"Reply with exactly {BASIC_EXPECTED_AGENT_REPLY} and no other text."
                )
                runner_turn_submission_count = 1
                lifecycle_stage = "turn_submission_attempted"
                turn_response = server.request(
                    4,
                    "turn/start",
                    {
                        "input": [
                            {
                                "text": prompt,
                                "textElements": [],
                                "type": "text",
                            }
                        ],
                        "threadId": thread_id,
                    },
                )
                lifecycle_stage = "turn_start_response_received"
                turn_id = require_turn_start_identity(
                    turn_response, "the single basic Grok Turn"
                )
                lifecycle_stage = "turn_submitted"
                turn_evidence = wait_for_basic_turn(
                    server,
                    time.monotonic() + BASIC_TURN_SECONDS,
                    thread_id,
                    turn_id,
                )
            elif scenario == CONTINUATION_SCENARIO:
                prompt = (
                    f"Use the {TOOL_NAME} result, then reply "
                    f"with exactly {EXPECTED_AGENT_REPLY} and no other text."
                )
                runner_turn_submission_count = 1
                lifecycle_stage = "turn_submission_attempted"
                turn_response = server.request(
                    4,
                    "turn/start",
                    {
                        "input": [
                            {
                                "text": prompt,
                                "textElements": [],
                                "type": "text",
                            }
                        ],
                        "threadId": thread_id,
                    },
                )
                lifecycle_stage = "turn_start_response_received"
                turn_id = require_turn_start_identity(
                    turn_response, "the Grok tool-continuation Turn"
                )
                lifecycle_stage = "turn_submitted"
                turn_evidence = wait_for_verified_turn(
                    server,
                    time.monotonic() + CONTINUATION_TURN_SECONDS,
                    thread_id,
                    turn_id,
                )
                lifecycle_stage = "first_turn_completed"

                history_prompt = (
                    "Reply with exactly the result returned by grokex_live_probe in the "
                    "previous Turn and no other text. Do not call any tool."
                )
                runner_turn_submission_count = 2
                lifecycle_stage = "history_turn_submission_attempted"
                history_turn_response = server.request(
                    5,
                    "turn/start",
                    {
                        "input": [
                            {
                                "text": history_prompt,
                                "textElements": [],
                                "type": "text",
                            }
                        ],
                        "threadId": thread_id,
                    },
                )
                lifecycle_stage = "history_turn_start_response_received"
                history_turn_id = require_turn_start_identity(
                    history_turn_response,
                    "the Grok history-replay Turn",
                    frozenset({turn_id}),
                )
                lifecycle_stage = "history_turn_submitted"
                history_evidence = wait_for_terminal_reply(
                    server,
                    time.monotonic() + CONTINUATION_TURN_SECONDS,
                    thread_id,
                    history_turn_id,
                    HISTORY_EXPECTED_AGENT_REPLY,
                    "the Grok history-replay Turn",
                )
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
                lifecycle_stage = "turn_submission_attempted"
                turn_response = server.request(
                    4,
                    "turn/start",
                    {
                        "effort": "ultra",
                        "input": [
                            {
                                "text": prompt,
                                "textElements": [],
                                "type": "text",
                            }
                        ],
                        "threadId": thread_id,
                    },
                )
                lifecycle_stage = "turn_start_response_received"
                turn_id = require_turn_start_identity(
                    turn_response, "the Grok Ultra collaboration Turn"
                )
                lifecycle_stage = "turn_submitted"
                turn_evidence, completed_child_thread_ids = wait_for_collaboration_turn(
                    server,
                    time.monotonic() + COLLABORATION_TURN_SECONDS,
                    thread_id,
                    turn_id,
                )
                lifecycle_stage = "collaboration_completed"
                for offset, child_thread_id in enumerate(
                    sorted(completed_child_thread_ids), start=10
                ):
                    lifecycle_stage = "child_binding_verification_attempted"
                    child_response = server.request(
                        offset,
                        "thread/resume",
                        {"excludeTurns": True, "threadId": child_thread_id},
                    )
                    child_thread = child_response.get("thread")
                    if (
                        not isinstance(child_thread, dict)
                        or child_thread.get("id") != child_thread_id
                        or child_response.get("modelProvider") != "grok"
                        or child_response.get("model") != "grok-4.6"
                    ):
                        raise LiveScenarioFailed(
                            "the completed child provider binding",
                            {
                                **turn_evidence,
                                "child_provider_binding_verified": False,
                                "last_proven_stage": "child_binding_response_received",
                            },
                            "semantic_contract",
                            "semantic_failure",
                        )
                lifecycle_stage = "child_binding_verified"
                turn_evidence["child_provider_binding"] = "grok/grok-4.6"
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
                    lifecycle_stage = (
                        "history_turn_submission_attempted"
                        if require_history
                        else "turn_submission_attempted"
                    )
                    turn_response = server.request(
                        request_id,
                        "turn/start",
                        {
                            "input": [
                                {
                                    "text": prompt,
                                    "textElements": [],
                                    "type": "text",
                                }
                            ],
                            "threadId": thread_id,
                        },
                    )
                    lifecycle_stage = (
                        "history_turn_start_response_received"
                        if require_history
                        else "turn_start_response_received"
                    )
                    turn_id = require_turn_start_identity(
                        turn_response,
                        "the Grok image Turn",
                        frozenset(image_turn_ids),
                    )
                    image_turn_ids.add(turn_id)
                    lifecycle_stage = (
                        "history_turn_submitted" if require_history else "turn_submitted"
                    )
                    image_evidence, artifacts = wait_for_image_turn(
                        server,
                        time.monotonic() + IMAGE_TURN_SECONDS,
                        thread_id,
                        turn_id,
                        require_history,
                        generation_artifacts,
                    )
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
                "archive": archive.name,
                "archive_sha256": sha256(archive),
                "catalog": "release-bundled",
                "model": thread_model,
                "runner_turn_submission_count": runner_turn_submission_count,
                "provider": "grok",
                "scenario": scenario,
                "source_sha": source_sha,
                **(
                    {"multi_agent_version": "v2", "reasoning_effort": "ultra"}
                    if scenario == COLLABORATION_SCENARIO
                    else {}
                ),
                **turn_evidence,
                "story": STORY_BY_SCENARIO[scenario],
                "validation_run": run_id,
                "validator_sha": validator_sha,
            }
            evidence_path.write_text(
                json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8"
            )
        except SystemExit as error:
            if isinstance(error, LiveScenarioFailed):
                failure_evidence = dict(error.last_stage)
                failure_evidence.setdefault("last_proven_stage", lifecycle_stage)
            else:
                failure_evidence = {
                    "does_not_prove": "product_root_cause",
                    "failure_category": "app_server_or_semantic_contract",
                    "last_proven_stage": lifecycle_stage,
                    "outcome": "semantic_failure",
                }
            evidence = {
                "archive": archive.name,
                "archive_sha256": sha256(archive),
                "catalog": "release-bundled",
                "model": thread_model,
                "provider": "grok",
                "runner_turn_submission_count": runner_turn_submission_count,
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
                **failure_evidence,
            }
            evidence_path.write_text(
                json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8"
            )
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
    args = parser.parse_args()
    run_smoke(
        args.archive,
        args.config,
        args.evidence,
        args.source_sha,
        args.validator_sha,
        args.run_id,
        args.scenario,
    )


if __name__ == "__main__":
    main()
