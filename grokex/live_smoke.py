#!/usr/bin/env python3
"""Run one bounded, secret-safe Grok scenario through a packaged App Server."""

import argparse
from collections import Counter
from collections import deque
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
from pathlib import Path


TAG = "grokex-v0.149.0"
BASIC_SCENARIO = "basic-exact-reply"
CONTINUATION_SCENARIO = "encrypted-reasoning-tool-continuation"
COLLABORATION_SCENARIO = "ultra-full-history-collaboration"
SCENARIOS = (BASIC_SCENARIO, CONTINUATION_SCENARIO, COLLABORATION_SCENARIO)
BASIC_EXPECTED_AGENT_REPLY = "GROKEX_BASIC_RESPONSE_OK"
TOOL_NAME = "grokex_live_probe"
TOOL_OUTPUT_MARKER = "GROKEX_LIVE_TOOL_OK"
EXPECTED_AGENT_REPLY = "GROKEX_LIVE_RESPONSE_OK"
HISTORY_EXPECTED_AGENT_REPLY = "GROKEX_HISTORY_RESPONSE_OK"
CHILD_EXPECTED_AGENT_REPLY = "GROKEX_ULTRA_CHILD_OK"
PARENT_EXPECTED_AGENT_REPLY = "GROKEX_ULTRA_PARENT_OK"


class AppServerDeadline(RuntimeError):
    def __init__(self, waiting_for: str) -> None:
        super().__init__(waiting_for)
        self.waiting_for = waiting_for


class ScenarioFailure(SystemExit):
    def __init__(self, message: str, evidence: dict[str, object]) -> None:
        super().__init__(message)
        self.evidence = evidence


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

    def request(
        self,
        request_id: int,
        method: str,
        params: dict[str, object],
        timeout_seconds: float = 30,
    ) -> dict[str, object]:
        self.send({"id": request_id, "method": method, "params": params})
        deadline = time.monotonic() + timeout_seconds
        while time.monotonic() < deadline:
            message = self._next_transport_message(deadline, method)
            if message.get("id") != request_id:
                self.deferred_messages.append(message)
                continue
            if "error" in message:
                raise SystemExit(f"App Server rejected {method}")
            response = message.get("response", message.get("result"))
            if not isinstance(response, dict):
                raise SystemExit(f"App Server returned an invalid {method} response")
            return response
        raise SystemExit(f"App Server timed out during {method}")

    def _next_transport_message(
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
            raise AppServerDeadline(waiting_for) from error

    def next_message(self, deadline: float, waiting_for: str) -> dict[str, object]:
        if self.deferred_messages:
            return self.deferred_messages.popleft()
        return self._next_transport_message(deadline, waiting_for)

    def close(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)


def wait_for_exact_reply(
    server: AppServer,
    deadline: float,
    expected_reply: str,
    turn_name: str,
) -> dict[str, str]:
    agent_reply = None
    status = None

    while time.monotonic() < deadline:
        message = server.next_message(deadline, turn_name)
        method = message.get("method")
        params = message.get("params")
        if not isinstance(params, dict):
            continue
        if method == "item/completed":
            item = params.get("item")
            if isinstance(item, dict) and item.get("type") == "agentMessage":
                agent_reply = item.get("text")
        elif method == "turn/completed":
            turn = params.get("turn")
            status = turn.get("status") if isinstance(turn, dict) else None
            break

    if status != "completed":
        safe_status = status if isinstance(status, str) else "missing"
        raise SystemExit(
            f"{turn_name} did not complete "
            f"(status={safe_status}, "
            f"agent_reply_seen={str(isinstance(agent_reply, str)).lower()})"
        )
    if not isinstance(agent_reply, str) or agent_reply.strip() != expected_reply:
        raise SystemExit(f"{turn_name} did not return the expected semantic reply")
    return {
        "response_assertion": "exact_match",
        "status": status,
    }


def wait_for_basic_turn(server: AppServer, deadline: float) -> dict[str, str]:
    return wait_for_exact_reply(
        server,
        deadline,
        BASIC_EXPECTED_AGENT_REPLY,
        "the single basic Grok Turn",
    )


def wait_for_verified_turn(server: AppServer, deadline: float) -> dict[str, str]:
    reasoning_completed = False
    tool_request_count = 0
    tool_completed = False
    agent_reply = None
    status = None

    while time.monotonic() < deadline:
        message = server.next_message(deadline, "the single Grok Turn")
        method = message.get("method")
        params = message.get("params")
        if method == "item/tool/call":
            request_id = message.get("id")
            if not isinstance(request_id, (int, str)) or not isinstance(params, dict):
                raise SystemExit("the Grok Turn returned an invalid dynamic tool request")
            if params.get("tool") != TOOL_NAME or params.get("arguments") != {}:
                raise SystemExit("the Grok Turn requested an unexpected dynamic tool operation")
            tool_request_count += 1
            if tool_request_count != 1:
                raise SystemExit("the Grok Turn requested the semantic tool more than once")
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
        if not isinstance(params, dict):
            continue
        if method == "item/completed":
            item = params.get("item")
            if not isinstance(item, dict):
                continue
            item_type = item.get("type")
            if item_type == "reasoning":
                reasoning_completed = True
            elif item_type == "dynamicToolCall":
                content_items = item.get("contentItems")
                tool_completed = (
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
            elif item_type == "agentMessage":
                agent_reply = item.get("text")
        elif method == "turn/completed":
            turn = params.get("turn")
            status = turn.get("status") if isinstance(turn, dict) else None
            break

    if status != "completed":
        safe_status = status if isinstance(status, str) else "missing"
        raise SystemExit(
            "the single Grok Turn did not complete "
            f"(status={safe_status}, "
            f"reasoning_completed={str(reasoning_completed).lower()}, "
            f"tool_requests={tool_request_count}, "
            f"tool_completed={str(tool_completed).lower()}, "
            f"agent_reply_seen={str(isinstance(agent_reply, str)).lower()})"
        )
    if not reasoning_completed:
        raise SystemExit("the Grok Turn did not expose a completed reasoning item")
    if tool_request_count != 1 or not tool_completed:
        raise SystemExit("the Grok Turn did not complete one semantic tool continuation")
    if not isinstance(agent_reply, str) or agent_reply.strip() != EXPECTED_AGENT_REPLY:
        raise SystemExit("the Grok Turn did not return the expected semantic reply")
    return {
        "response_assertion": "exact_match",
        "status": status,
        "tool_continuation": "completed",
    }


def summarize_thread_read(
    response: dict[str, object],
    expected_reply: str,
) -> dict[str, object]:
    thread = response.get("thread")
    if not isinstance(thread, dict):
        return {"read_status": "invalid"}

    status = thread.get("status")
    status_type = status.get("type") if isinstance(status, dict) else None
    allowed_thread_statuses = {"active", "idle", "notLoaded", "systemError"}
    thread_status = (
        status_type if status_type in allowed_thread_statuses else "unknown"
    )

    turns = thread.get("turns")
    if not isinstance(turns, list):
        turns = []
    turn_status_counts: Counter[str] = Counter()
    item_type_counts: Counter[str] = Counter()
    expected_reply_index = None
    wait_completed_seen = False
    item_index = 0
    allowed_turn_statuses = {"completed", "failed", "inProgress", "interrupted"}
    allowed_item_types = {
        "agentMessage",
        "collabAgentToolCall",
        "reasoning",
        "subAgentActivity",
    }
    for turn in turns:
        if not isinstance(turn, dict):
            continue
        turn_status = turn.get("status")
        turn_status_counts[
            turn_status if turn_status in allowed_turn_statuses else "unknown"
        ] += 1
        items = turn.get("items")
        if not isinstance(items, list):
            continue
        for item in items:
            item_type = item.get("type") if isinstance(item, dict) else None
            item_type_counts[
                item_type if item_type in allowed_item_types else "other"
            ] += 1
            if isinstance(item, dict) and item_type == "agentMessage":
                text = item.get("text")
                if isinstance(text, str) and text.strip() == expected_reply:
                    expected_reply_index = item_index
            elif isinstance(item, dict) and item_type == "collabAgentToolCall":
                if item.get("tool") == "wait" and item.get("status") == "completed":
                    wait_completed_seen = True
            item_index += 1
    return {
        "expected_reply_seen": expected_reply_index is not None,
        "item_type_counts": dict(sorted(item_type_counts.items())),
        "provider_match": thread.get("modelProvider") == "grok",
        "read_status": "completed",
        "thread_status": thread_status,
        "latest_turn_status": (
            turns[-1].get("status") if isinstance(turns[-1], dict) else "unknown"
        ) if turns else "missing",
        "turn_count": len(turns),
        "turn_status_counts": dict(sorted(turn_status_counts.items())),
        "wait_completed_seen": wait_completed_seen,
    }


def collect_thread_snapshots(
    server: AppServer,
    root_thread_id: str,
    child_thread_ids: set[str],
) -> tuple[dict[str, object], dict[str, dict[str, object]]]:
    roles = [("parent", root_thread_id)] + [
        ("child", thread_id) for thread_id in sorted(child_thread_ids)
    ]
    parent_snapshot: dict[str, object] = {"read_status": "unavailable"}
    child_snapshots: dict[str, dict[str, object]] = {}
    for offset, (role, thread_id) in enumerate(roles):
        try:
            response = server.request(
                9000 + offset,
                "thread/read",
                {"includeTurns": True, "threadId": thread_id},
                timeout_seconds=3,
            )
            expected = (
                PARENT_EXPECTED_AGENT_REPLY
                if role == "parent"
                else CHILD_EXPECTED_AGENT_REPLY
            )
            summary = summarize_thread_read(response, expected)
        except (AppServerDeadline, SystemExit):
            summary = {"read_status": "unavailable"}
        if role == "child":
            child_snapshots[thread_id] = summary
        else:
            parent_snapshot = summary
    return parent_snapshot, child_snapshots


def wait_for_collaboration_turn(
    server: AppServer,
    deadline: float,
    root_thread_id: str,
) -> dict[str, object]:
    started_at = time.monotonic()
    child_replies: dict[str, bool] = {}
    child_reply_ms: dict[str, int] = {}
    child_statuses: dict[str, object] = {}
    child_completed_ms: dict[str, int] = {}
    child_started_ms: dict[str, int] = {}
    runtime_child_ids: set[str] = set()
    runtime_child_paths: dict[str, str] = {}
    runtime_spawn_models: dict[str, str] = {}
    runtime_spawn_receivers: dict[str, set[str]] = {}
    spawn_call_ids: set[str] = set()
    spawn_expected_model_call_ids: set[str] = set()
    provider_wait_call_ids: set[str] = set()
    default_history_spawn_seen = False
    missing_response_identity_count = 0
    other_tool_completed_count = 0
    parent_reply_seen = False
    parent_result_authors: set[str] = set()
    response_ids_by_thread: dict[str, set[str]] = {}
    root_status = None
    spawn_completed_call_ids: set[str] = set()
    spawn_failed_call_ids: set[str] = set()
    spawn_completed_ms = None
    spawn_requested_ms = None
    wait_completed_call_ids: set[str] = set()
    wait_failed_call_ids: set[str] = set()
    wait_completed_ms = None
    wait_started_call_ids: set[str] = set()
    wait_started_ms = None
    deadline_reached = False
    parent_completed_ms = None
    parent_reply_ms = None

    def elapsed_ms() -> int:
        return int((time.monotonic() - started_at) * 1000)

    def target_runtime_child_ids() -> set[str]:
        target_ids: set[str] = set()
        for call_id in spawn_call_ids:
            target_ids.update(runtime_spawn_receivers.get(call_id, set()))
        return target_ids

    def completed_target_children() -> set[str]:
        return {
            thread_id
            for thread_id in target_runtime_child_ids()
            if child_statuses.get(thread_id) == "completed"
            and child_replies.get(thread_id) is True
        }

    def correlated_wait_call_ids() -> set[str]:
        return (
            provider_wait_call_ids
            & wait_started_call_ids
            & wait_completed_call_ids
        )

    def semantic_complete() -> bool:
        return (
            root_status == "completed"
            and parent_reply_seen
            and default_history_spawn_seen
            and any(
                runtime_child_paths.get(thread_id) in parent_result_authors
                for thread_id in completed_target_children()
            )
        )

    try:
        while time.monotonic() < deadline:
            if semantic_complete():
                break
            message = server.next_message(deadline, "the Grok Ultra collaboration Turn")
            method = message.get("method")
            params = message.get("params")
            if not isinstance(params, dict):
                continue
            thread_id = params.get("threadId")

            if method == "rawResponse/completed" and isinstance(thread_id, str):
                response_id = params.get("responseId")
                if not isinstance(response_id, str) or not response_id:
                    missing_response_identity_count += 1
                else:
                    response_ids_by_thread.setdefault(thread_id, set()).add(response_id)
                continue

            if method == "rawResponseItem/completed" and thread_id == root_thread_id:
                item = params.get("item")
                if not isinstance(item, dict):
                    continue
                if item.get("type") == "agent_message":
                    author = item.get("author")
                    recipient = item.get("recipient")
                    content = item.get("content")
                    if (
                        isinstance(author, str)
                        and isinstance(recipient, str)
                        and isinstance(content, list)
                        and len(content) == 1
                        and isinstance(content[0], dict)
                        and content[0].get("type") == "input_text"
                        and content[0].get("text")
                        == (
                            "Message Type: FINAL_ANSWER\n"
                            f"Task name: {recipient}\nSender: {author}\nPayload:\n"
                            f"{CHILD_EXPECTED_AGENT_REPLY}"
                        )
                    ):
                        parent_result_authors.add(author)
                    continue
                if item.get("type") != "function_call":
                    continue
                name = item.get("name")
                call_id = item.get("call_id")
                if name == "wait_agent":
                    if isinstance(call_id, str) and call_id:
                        provider_wait_call_ids.add(call_id)
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
                task = parsed_arguments.get("message")
                if not isinstance(task, str) or CHILD_EXPECTED_AGENT_REPLY not in task:
                    continue
                if parsed_arguments.get("fork_turns", "all") != "all":
                    raise SystemExit(
                        "the Grok collaboration spawn did not use full history"
                    )
                default_history_spawn_seen = True
                if spawn_requested_ms is None:
                    spawn_requested_ms = elapsed_ms()
                if isinstance(call_id, str) and call_id:
                    spawn_call_ids.add(call_id)
                    requested_model = parsed_arguments.get("model")
                    if requested_model is None or requested_model == "grok-4.6":
                        spawn_expected_model_call_ids.add(call_id)
                continue

            if method in {"item/completed", "item/started"}:
                item = params.get("item")
                if not isinstance(item, dict):
                    continue
                if (
                    thread_id == root_thread_id
                    and item.get("type") == "collabAgentToolCall"
                ):
                    tool = item.get("tool")
                    call_id = item.get("id")
                    if method == "item/started":
                        if tool == "wait":
                            if isinstance(call_id, str) and call_id:
                                wait_started_call_ids.add(call_id)
                            if wait_started_ms is None:
                                wait_started_ms = elapsed_ms()
                        continue
                    status = item.get("status")
                    receiver_thread_ids = item.get("receiverThreadIds")
                    receivers = {
                        receiver
                        for receiver in receiver_thread_ids
                        if isinstance(receiver, str) and receiver
                    } if isinstance(receiver_thread_ids, list) else set()
                    if tool == "spawnAgent":
                        if status == "failed":
                            if isinstance(call_id, str) and call_id:
                                spawn_failed_call_ids.add(call_id)
                            continue
                        if status != "completed":
                            continue
                        runtime_child_ids.update(receivers)
                        if isinstance(call_id, str) and call_id:
                            first_completion = call_id not in spawn_completed_call_ids
                            spawn_completed_call_ids.add(call_id)
                            if first_completion and spawn_completed_ms is None:
                                spawn_completed_ms = elapsed_ms()
                            runtime_spawn_receivers.setdefault(call_id, set()).update(
                                receivers
                            )
                            model = item.get("model")
                            if isinstance(model, str):
                                runtime_spawn_models[call_id] = model
                    elif tool == "wait":
                        if status == "failed":
                            if isinstance(call_id, str) and call_id:
                                wait_failed_call_ids.add(call_id)
                            continue
                        if status != "completed":
                            continue
                        if isinstance(call_id, str) and call_id:
                            wait_completed_call_ids.add(call_id)
                        if wait_completed_ms is None:
                            wait_completed_ms = elapsed_ms()
                    else:
                        if status != "completed":
                            continue
                        other_tool_completed_count += 1
                elif item.get("type") == "agentMessage" and isinstance(thread_id, str):
                    reply = item.get("text")
                    if thread_id == root_thread_id:
                        parent_reply_seen = reply == PARENT_EXPECTED_AGENT_REPLY
                        if parent_reply_seen and parent_reply_ms is None:
                            parent_reply_ms = elapsed_ms()
                    else:
                        child_replies[thread_id] = reply == CHILD_EXPECTED_AGENT_REPLY
                        if child_replies[thread_id]:
                            child_reply_ms.setdefault(thread_id, elapsed_ms())
                elif item.get("type") == "subAgentActivity":
                    child_thread_id = item.get("agentThreadId")
                    if isinstance(child_thread_id, str) and child_thread_id:
                        runtime_child_ids.add(child_thread_id)
                        child_path = item.get("agentPath")
                        if isinstance(child_path, str) and child_path:
                            runtime_child_paths[child_thread_id] = child_path
                        if item.get("kind") == "started":
                            child_started_ms.setdefault(child_thread_id, elapsed_ms())
                            call_id = item.get("id")
                            if (
                                isinstance(call_id, str)
                                and call_id in spawn_call_ids
                            ):
                                first_completion = (
                                    call_id not in spawn_completed_call_ids
                                )
                                spawn_completed_call_ids.add(call_id)
                                runtime_spawn_receivers.setdefault(
                                    call_id, set()
                                ).add(child_thread_id)
                                if first_completion and spawn_completed_ms is None:
                                    spawn_completed_ms = elapsed_ms()
                continue

            if method == "turn/completed" and isinstance(thread_id, str):
                turn = params.get("turn")
                status = turn.get("status") if isinstance(turn, dict) else None
                if thread_id == root_thread_id:
                    root_status = status
                    if status in {"completed", "failed", "interrupted"}:
                        parent_completed_ms = elapsed_ms()
                else:
                    child_statuses[thread_id] = status
                    if status in {"completed", "failed", "interrupted"}:
                        child_completed_ms[thread_id] = elapsed_ms()
        else:
            deadline_reached = True
    except AppServerDeadline:
        deadline_reached = True

    known_target_children = target_runtime_child_ids()
    parent_snapshot, child_snapshots = collect_thread_snapshots(
        server,
        root_thread_id,
        runtime_child_ids,
    )
    def completed(thread_id: str) -> bool:
        snapshot = child_snapshots.get(thread_id, {})
        return (
            child_statuses.get(thread_id) == "completed"
            and child_replies.get(thread_id) is True
        ) or (
            deadline_reached
            and snapshot.get("expected_reply_seen") is True
            and snapshot.get("latest_turn_status") == "completed"
        )

    def model_matches(thread_id: str) -> bool:
        return any(
            thread_id in runtime_spawn_receivers.get(call_id, set())
            and (
                runtime_spawn_models.get(call_id) == "grok-4.6"
                if call_id in runtime_spawn_models
                else call_id in spawn_expected_model_call_ids
            )
            for call_id in spawn_call_ids
        )

    semantic_candidates = {
        thread_id
        for thread_id in known_target_children
        if completed(thread_id)
        and runtime_child_paths.get(thread_id) in parent_result_authors
        and model_matches(thread_id)
        and child_snapshots.get(thread_id, {}).get("provider_match") is True
    }
    target_child_id = next(
        iter(sorted(semantic_candidates or known_target_children)), None
    )
    target_snapshot = child_snapshots.get(target_child_id, {})
    target_model_match = (
        model_matches(target_child_id) if target_child_id is not None else False
    )
    other_snapshots = [
        child_snapshots[thread_id]
        for thread_id in sorted(runtime_child_ids - {target_child_id})
    ]
    thread_snapshots = {
        "other_children": other_snapshots,
        "parent": parent_snapshot,
        "target_child": target_snapshot,
    }

    target_child_completed = (
        completed(target_child_id) if target_child_id is not None else False
    )
    parent_completed = (
        root_status == "completed"
        or (
            parent_snapshot.get("expected_reply_seen") is True
            and parent_snapshot.get("latest_turn_status") == "completed"
        )
    )
    parent_reply_observed = (
        parent_reply_seen or parent_snapshot.get("expected_reply_seen") is True
    )
    target_child_path = runtime_child_paths.get(target_child_id)
    parent_consumed_result = (
        target_child_path in parent_result_authors and parent_reply_observed
    )
    wait_correlated_to_target = (
        bool(correlated_wait_call_ids())
        and target_child_completed
        and parent_consumed_result
    )
    parent_response_count = len(response_ids_by_thread.get(root_thread_id, set()))
    target_child_response_count = (
        len(response_ids_by_thread.get(target_child_id, set()))
        if target_child_id is not None
        else 0
    )
    other_child_response_count = sum(
        len(response_ids)
        for thread_id, response_ids in response_ids_by_thread.items()
        if thread_id not in {root_thread_id, target_child_id}
    )
    observations: dict[str, object] = {
        "deadline_reached": deadline_reached,
        "elapsed_ms": elapsed_ms(),
        "missing_response_identity_count": missing_response_identity_count,
        "other_child_response_count": other_child_response_count,
        "other_tool_completed_count": other_tool_completed_count,
        "parent_reply_seen": parent_reply_observed,
        "parent_reply_ms": parent_reply_ms,
        "parent_response_count": parent_response_count,
        "parent_completed_ms": parent_completed_ms,
        "parent_turn_status": (
            root_status
            if isinstance(root_status, str)
            else parent_snapshot.get("latest_turn_status", "missing")
        ),
        "parent_result_consumed": parent_consumed_result,
        "provider_spawn_request_count": len(spawn_call_ids),
        "provider_wait_request_count": len(provider_wait_call_ids),
        "runtime_child_count": len(runtime_child_ids),
        "runtime_spawn_completed_count": len(spawn_completed_call_ids),
        "runtime_spawn_failed_count": len(spawn_failed_call_ids),
        "spawn_completed_ms": spawn_completed_ms,
        "spawn_requested_ms": spawn_requested_ms,
        "target_child_completed_ms": child_completed_ms.get(target_child_id),
        "target_runtime_child_count": len(target_runtime_child_ids()),
        "target_child_reply_ms": child_reply_ms.get(target_child_id),
        "target_child_reply_seen": target_child_completed,
        "target_child_response_count": target_child_response_count,
        "target_child_started_ms": child_started_ms.get(target_child_id),
        "target_child_turn_status": (
            child_statuses.get(
                target_child_id,
                target_snapshot.get("latest_turn_status", "missing"),
            ) if target_child_id is not None else "missing"
        ),
        "target_model_match": target_model_match,
        "target_provider_match": target_snapshot.get("provider_match") is True,
        "thread_snapshots": thread_snapshots,
        "wait_completed_count": len(wait_completed_call_ids),
        "wait_completed_ms": wait_completed_ms,
        "wait_correlated_call_count": len(correlated_wait_call_ids()),
        "wait_correlated_to_target": wait_correlated_to_target,
        "wait_failed_count": len(wait_failed_call_ids),
        "wait_started_count": len(wait_started_call_ids),
        "wait_started_ms": wait_started_ms,
        "wait_timed_out_if_observable": "not_observed",
    }
    failures: list[str] = []
    if deadline_reached:
        failures.append("the bounded Grok Ultra deadline was reached")
    if not default_history_spawn_seen or not target_runtime_child_ids():
        failures.append("the Grok Ultra Turn did not prove a default-history runtime child")
    if target_runtime_child_ids() and not target_child_completed:
        failures.append("the Grok Ultra target child did not complete the bounded task")
    if not parent_completed:
        failures.append("the Grok Ultra parent Turn did not complete")
    if not parent_consumed_result:
        failures.append("the Grok Ultra parent did not consume the child result and continue")
    if not target_model_match or target_snapshot.get("provider_match") is not True:
        failures.append("the Grok Ultra target child did not preserve Provider/model ownership")
    if failures:
        trajectory: dict[str, str] = {}
        if deadline_reached:
            if parent_completed:
                last_proven_stage = "parent_completed"
            elif target_child_completed:
                last_proven_stage = "target_child_completed"
            elif target_runtime_child_ids():
                last_proven_stage = "runtime_child_created"
            elif spawn_call_ids:
                last_proven_stage = "provider_spawn_requested"
            else:
                last_proven_stage = "parent_turn_observed"
            if not default_history_spawn_seen or not target_runtime_child_ids():
                trajectory_gap = "default_history_runtime_child"
            elif not target_child_completed:
                trajectory_gap = "target_child_completion"
            elif not parent_completed:
                trajectory_gap = "parent_completion"
            elif not parent_consumed_result:
                trajectory_gap = "parent_result_consumption"
            elif not target_model_match or target_snapshot.get("provider_match") is not True:
                trajectory_gap = "provider_model_ownership"
            else:
                trajectory_gap = "semantic_terminal_proof"
            trajectory = {
                "last_proven_stage": last_proven_stage,
                "trajectory_gap": trajectory_gap,
            }
        if not deadline_reached:
            root_cause_classification = "semantic_contract_not_proven"
            oracle_sufficiency = "sufficient"
        elif spawn_call_ids & spawn_failed_call_ids and not target_runtime_child_ids():
            root_cause_classification = "runtime_spawn"
            oracle_sufficiency = "sufficient"
        else:
            root_cause_classification = "inconclusive"
            oracle_sufficiency = "insufficient"
        raise ScenarioFailure(
            "; ".join(failures) + "; semantic proof is incomplete",
            {
                **trajectory,
                "observations": observations,
                "oracle_sufficiency": oracle_sufficiency,
                "root_cause_classification": root_cause_classification,
                "semantic_acceptance": "not_proven",
                "status": "failed",
            },
        )
    return {
        "child_completion": "completed",
        "child_response_assertion": "exact_match",
        "default_full_history": "completed",
        "parent_completion": "completed",
        "parent_result_consumption": "completed",
        "observations": observations,
        "response_assertion": "exact_match",
        "semantic_acceptance": "proven",
        "status": "completed",
    }


def run_smoke(
    archive: Path,
    config: Path,
    evidence_path: Path,
    source_sha: str,
    validator_sha: str,
    run_id: str,
    scenario: str,
) -> None:
    with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as temporary:
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

        base_evidence = {
            "archive": archive.name,
            "archive_sha256": sha256(archive),
            "catalog": "release-bundled",
            "model": "grok-4.6",
            "multi_agent_version": "v2",
            "provider": "grok",
            "reasoning_effort": "ultra",
            "scenario": scenario,
            "source_sha": source_sha,
            "story": f"grokex-{scenario}",
            "validation_run": run_id,
            "validator_sha": validator_sha,
        }
        server = AppServer(root / "bin/grokex-bin", codex_home, workspace, token)
        try:
            server.request(
                1,
                "initialize",
                {
                    "clientInfo": {"name": "grokex-release", "version": "0.149.0"},
                    "capabilities": {"experimentalApi": True},
                },
            )
            server.send({"method": "initialized"})
            models_response = server.request(
                2, "model/list", {"cursor": None, "includeHidden": None, "limit": 100}
            )
            models = models_response.get("data")
            if not isinstance(models, list):
                raise SystemExit("model/list did not return a catalog")
            matching = [
                model
                for model in models
                if isinstance(model, dict) and model.get("id") == "grok-4.6"
            ]
            if len(matching) != 1:
                raise SystemExit("release catalog does not contain exact grok-4.6")
            model = matching[0]
            efforts = {
                option.get("reasoningEffort")
                for option in model.get("supportedReasoningEfforts", [])
                if isinstance(option, dict)
            }
            if model.get("multiAgentVersion") != "v2" or "ultra" not in efforts:
                raise SystemExit("grok-4.6 release metadata is incomplete")

            thread_params: dict[str, object] = {
                "cwd": str(workspace),
                "ephemeral": scenario != COLLABORATION_SCENARIO,
                "model": "grok-4.6",
                "modelProvider": "grok",
            }
            if scenario == COLLABORATION_SCENARIO:
                thread_params["experimentalRawEvents"] = True
            if scenario == CONTINUATION_SCENARIO:
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
            thread_response = server.request(3, "thread/start", thread_params)
            thread = thread_response.get("thread")
            if not isinstance(thread, dict) or thread_response.get("modelProvider") != "grok":
                raise SystemExit("thread/start did not bind Provider grok")
            thread_id = thread.get("id")
            if not isinstance(thread_id, str) or not thread_id:
                raise SystemExit("thread/start returned no thread identity")

            runner_turn_submission_count = 1
            if scenario == BASIC_SCENARIO:
                prompt = f"Reply with exactly {BASIC_EXPECTED_AGENT_REPLY} and no other text."
                request = {"input": [{"text": prompt, "textElements": [], "type": "text"}], "threadId": thread_id}
                server.request(4, "turn/start", request)
                turn_evidence = wait_for_basic_turn(server, time.monotonic() + 120)
            elif scenario == CONTINUATION_SCENARIO:
                prompt = (
                    f"Call {TOOL_NAME} exactly once. Use its result, then reply "
                    f"with exactly {EXPECTED_AGENT_REPLY} and no other text."
                )
                request = {"input": [{"text": prompt, "textElements": [], "type": "text"}], "threadId": thread_id}
                server.request(4, "turn/start", request)
                turn_evidence = wait_for_verified_turn(server, time.monotonic() + 120)
                history_prompt = (
                    f"Reply with exactly {HISTORY_EXPECTED_AGENT_REPLY} and no other text. "
                    "Do not call any tool."
                )
                request["input"] = [{"text": history_prompt, "textElements": [], "type": "text"}]
                server.request(5, "turn/start", request)
                runner_turn_submission_count = 2
                history_evidence = wait_for_exact_reply(
                    server,
                    time.monotonic() + 120,
                    HISTORY_EXPECTED_AGENT_REPLY,
                    "the Grok history-replay Turn",
                )
                turn_evidence = {
                    **turn_evidence,
                    "history_response_assertion": history_evidence["response_assertion"],
                    "reasoning_replay": "completed",
                }
            else:
                prompt = (
                    "Delegate this bounded task to a child agent using the default full-history "
                    "behavior. Ask the child to reply with exactly "
                    f"{CHILD_EXPECTED_AGENT_REPLY} and no other text. Use the child's returned "
                    "result, then reply with exactly "
                    f"{PARENT_EXPECTED_AGENT_REPLY} and no other text."
                )
                request = {
                    "effort": "ultra",
                    "input": [{"text": prompt, "textElements": [], "type": "text"}],
                    "threadId": thread_id,
                }
                server.request(4, "turn/start", request)
                try:
                    turn_evidence = wait_for_collaboration_turn(
                        server, time.monotonic() + 120, thread_id
                    )
                except ScenarioFailure as error:
                    evidence_path.write_text(
                        json.dumps(
                            {
                                **base_evidence,
                                "runner_turn_submission_count": 1,
                                **error.evidence,
                            },
                            indent=2,
                            sort_keys=True,
                        )
                        + "\n",
                        encoding="utf-8",
                    )
                    raise

            evidence = {
                **base_evidence,
                "runner_turn_submission_count": runner_turn_submission_count,
                "semantic_acceptance": "proven",
                **turn_evidence,
            }
            evidence_path.write_text(
                json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8"
            )
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
