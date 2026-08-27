#!/usr/bin/env python3
"""Run one bounded, secret-safe Grok scenario through a packaged App Server."""

from __future__ import annotations

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
from collections import deque
from pathlib import Path


TAG = "grokex-v0.149.0"
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
BASIC_EXPECTED_AGENT_REPLY = "GROKEX_BASIC_RESPONSE_OK"
TOOL_NAME = "grokex_live_probe"
TOOL_OUTPUT_MARKER = "GROKEX_LIVE_TOOL_OK"
EXPECTED_AGENT_REPLY = "GROKEX_LIVE_RESPONSE_OK"
HISTORY_EXPECTED_AGENT_REPLY = "GROKEX_HISTORY_RESPONSE_OK"
CHILD_EXPECTED_AGENT_REPLY = "GROKEX_ULTRA_CHILD_OK"
PARENT_EXPECTED_AGENT_REPLY = "GROKEX_ULTRA_PARENT_OK"


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
            message = self.next_message(deadline, method)
            if message.get("id") != request_id:
                continue
            if "error" in message:
                raise SystemExit(f"App Server rejected {method}")
            response = message.get("response", message.get("result"))
            if not isinstance(response, dict):
                raise SystemExit(f"App Server returned an invalid {method} response")
            return response
        raise SystemExit(f"App Server timed out during {method}")

    def next_message(self, deadline: float, waiting_for: str) -> dict[str, object]:
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
            raise SystemExit(
                f"App Server response deadline expired while waiting for {waiting_for}"
            ) from error

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


def wait_for_collaboration_turn(
    server: AppServer,
    deadline: float,
    root_thread_id: str,
) -> dict[str, object]:
    child_reply_seen = False
    child_status = None
    child_thread_id = None
    default_spawn_count = 0
    parent_reply_seen = False
    response_counts: dict[str, int] = {}
    root_status = None
    spawn_completed_count = 0
    wait_completed_count = 0

    while time.monotonic() < deadline:
        if (
            root_status == "completed"
            and child_status == "completed"
            and child_reply_seen
            and parent_reply_seen
        ):
            break
        message = server.next_message(deadline, "the Grok Ultra collaboration Turn")
        method = message.get("method")
        params = message.get("params")
        if not isinstance(params, dict):
            continue
        thread_id = params.get("threadId")

        if method == "rawResponse/completed" and isinstance(thread_id, str):
            response_counts[thread_id] = response_counts.get(thread_id, 0) + 1
            if response_counts.get(root_thread_id, 0) > 3:
                raise SystemExit("the Grok Ultra parent used more than three responses")
            if child_thread_id is not None and response_counts.get(child_thread_id, 0) > 1:
                raise SystemExit("the Grok Ultra child used more than one response")
            continue

        if method == "rawResponseItem/completed" and thread_id == root_thread_id:
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
            task = parsed_arguments.get("message")
            if not isinstance(task, str) or CHILD_EXPECTED_AGENT_REPLY not in task:
                continue
            if "fork_turns" in parsed_arguments:
                raise SystemExit("the Grok collaboration spawn did not use default full history")
            default_spawn_count += 1
            if default_spawn_count != 1:
                raise SystemExit("the Grok collaboration Turn requested more than one child")
            continue

        if method == "item/completed":
            item = params.get("item")
            if not isinstance(item, dict):
                continue
            if (
                thread_id == root_thread_id
                and item.get("type") == "collabAgentToolCall"
            ):
                tool = item.get("tool")
                if item.get("status") != "completed":
                    raise SystemExit("a Grok collaboration tool did not complete")
                if tool == "spawnAgent":
                    receiver_thread_ids = item.get("receiverThreadIds")
                    if not isinstance(receiver_thread_ids, list) or len(receiver_thread_ids) != 1:
                        raise SystemExit(
                            "the Grok collaboration spawn returned an invalid child set"
                        )
                    child_thread_id = receiver_thread_ids[0]
                    if not isinstance(child_thread_id, str) or not child_thread_id:
                        raise SystemExit("the Grok collaboration spawn returned no child identity")
                    spawn_completed_count += 1
                    if spawn_completed_count != 1:
                        raise SystemExit(
                            "the Grok collaboration Turn completed more than one spawn"
                        )
                elif tool == "wait":
                    wait_completed_count += 1
                    if wait_completed_count != 1:
                        raise SystemExit(
                            "the Grok collaboration Turn completed more than one wait"
                        )
                else:
                    raise SystemExit("the Grok collaboration Turn used an unexpected tool")
            elif item.get("type") == "agentMessage":
                reply = item.get("text")
                if thread_id == root_thread_id:
                    parent_reply_seen = reply == PARENT_EXPECTED_AGENT_REPLY
                elif child_thread_id is not None and thread_id == child_thread_id:
                    child_reply_seen = reply == CHILD_EXPECTED_AGENT_REPLY
            continue

        if method == "turn/completed":
            turn = params.get("turn")
            status = turn.get("status") if isinstance(turn, dict) else None
            if thread_id == root_thread_id:
                root_status = status
            if child_thread_id is not None and thread_id == child_thread_id:
                child_status = status

    if root_status != "completed":
        raise SystemExit("the Grok Ultra parent Turn did not complete")
    if child_status != "completed" or not child_reply_seen:
        raise SystemExit("the Grok Ultra child did not complete the bounded task")
    if not parent_reply_seen:
        raise SystemExit("the Grok Ultra parent did not return the expected semantic reply")
    if default_spawn_count != 1 or spawn_completed_count != 1:
        raise SystemExit("the Grok Ultra Turn did not prove one default-history spawn")
    if wait_completed_count != 1:
        raise SystemExit("the Grok Ultra Turn did not complete exactly one wait")
    if child_thread_id is None:
        raise SystemExit("the Grok Ultra Turn did not identify one child")
    if set(response_counts) != {root_thread_id, child_thread_id}:
        raise SystemExit("the Grok Ultra Turn observed an unexpected response owner")
    operation_count = sum(response_counts.values())
    if response_counts != {root_thread_id: 3, child_thread_id: 1}:
        raise SystemExit("the Grok Ultra Turn did not use exactly four responses")
    return {
        "child_completion": "completed",
        "child_response_assertion": "exact_match",
        "default_full_history": "completed",
        "parent_completion": "completed",
        "operation_count": operation_count,
        "response_assertion": "exact_match",
        "spawn_count": 1,
        "status": root_status,
        "wait_count": 1,
    }


def wait_for_image_turn(
    server: AppServer, deadline: float, require_history: bool
) -> dict[str, object]:
    completed = 0
    history_args_seen = not require_history
    while time.monotonic() < deadline:
        message = server.next_message(deadline, "the Grok image Turn")
        params = message.get("params")
        if not isinstance(params, dict):
            continue
        if message.get("method") == "rawResponse/completed":
            item = params.get("response")
            if isinstance(item, dict) and item.get("type") == "response.output_item.done":
                output = item.get("item")
                if isinstance(output, dict) and output.get("type") == "function_call":
                    try:
                        arguments = json.loads(output.get("arguments", ""))
                    except (TypeError, json.JSONDecodeError):
                        arguments = None
                    history_args_seen = history_args_seen or (
                        isinstance(arguments, dict)
                        and arguments.get("num_last_images_to_include") == 1
                        and arguments.get("referenced_image_paths") is None
                    )
        elif message.get("method") == "item/completed":
            item = params.get("item")
            if isinstance(item, dict) and item.get("type") == "imageGeneration":
                result = item.get("result")
                saved_path = item.get("savedPath")
                if item.get("status") != "completed" or not isinstance(result, str):
                    raise SystemExit("image generation item was not completed")
                try:
                    signature = base64.b64decode(result, validate=True)[:3]
                except (ValueError, TypeError) as error:
                    raise SystemExit("image generation result was not valid base64") from error
                decoded = base64.b64decode(result, validate=True)
                if not (decoded.startswith(b"\xff\xd8") and decoded.endswith(b"\xff\xd9")):
                    raise SystemExit("image generation result was not JPEG")
                artifact = Path(saved_path) if isinstance(saved_path, str) else None
                if artifact is None or artifact.suffix != ".jpg" or not artifact.is_file():
                    raise SystemExit("image generation artifact was not JPEG")
                if artifact.stat().st_size > 32 * 1024 * 1024 or artifact.read_bytes() != decoded:
                    raise SystemExit("image generation artifact did not match result")
                completed += 1
        elif message.get("method") == "turn/completed":
            turn = params.get("turn")
            if (
                not isinstance(turn, dict)
                or turn.get("status") != "completed"
                or completed != 1
                or not history_args_seen
            ):
                raise SystemExit("Grok image Turn did not complete exactly one image")
            return {
                "artifact_match": True,
                "history_arguments_verified": require_history,
                "image_items_completed": 1,
                "image_mime": "image/jpeg",
                "artifact_extension": ".jpg",
                "status": "completed",
            }
    raise SystemExit("Grok image Turn timed out")


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
            efforts = {
                option.get("reasoningEffort")
                for option in model.get("supportedReasoningEfforts", [])
                if isinstance(option, dict)
            }
            if model.get("multiAgentVersion") != "v2" or "ultra" not in efforts:
                raise SystemExit("grok-4.6 release metadata is incomplete")

            thread_params: dict[str, object] = {
                "cwd": str(workspace),
                "ephemeral": True,
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

            if scenario == BASIC_SCENARIO:
                prompt = (
                    f"Reply with exactly {BASIC_EXPECTED_AGENT_REPLY} and no other text."
                )
                operation_count = 1
                server.request(
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
                turn_evidence = wait_for_basic_turn(server, time.monotonic() + 120)
            elif scenario == CONTINUATION_SCENARIO:
                prompt = (
                    f"Call {TOOL_NAME} exactly once. Use its result, then reply "
                    f"with exactly {EXPECTED_AGENT_REPLY} and no other text."
                )
                server.request(
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
                turn_evidence = wait_for_verified_turn(
                    server, time.monotonic() + 120
                )

                history_prompt = (
                    f"Reply with exactly {HISTORY_EXPECTED_AGENT_REPLY} and no other text. "
                    "Do not call any tool."
                )
                server.request(
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
                history_evidence = wait_for_exact_reply(
                    server,
                    time.monotonic() + 120,
                    HISTORY_EXPECTED_AGENT_REPLY,
                    "the Grok history-replay Turn",
                )
                operation_count = 2
                turn_evidence = {
                    **turn_evidence,
                    "history_response_assertion": history_evidence[
                        "response_assertion"
                    ],
                    "reasoning_replay": "completed",
                }
            elif scenario == COLLABORATION_SCENARIO:
                prompt = (
                    "Use spawn_agent exactly once with task_name live_child. Omit fork_turns "
                    "so the child uses the default full-history fork. Tell the child to reply "
                    f"with exactly {CHILD_EXPECTED_AGENT_REPLY} and no other text. Call "
                    "wait_agent exactly once after spawning the child, wait for that child "
                    "to complete, then reply with exactly "
                    f"{PARENT_EXPECTED_AGENT_REPLY} and no other text."
                )
                server.request(
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
                turn_evidence = wait_for_collaboration_turn(
                    server,
                    time.monotonic() + 120,
                    thread_id,
                )
                operation_count = turn_evidence.pop("operation_count")
            else:
                for request_id, prompt, require_history in [
                    (
                        4,
                        "Use image_gen.imagegen once to generate one JPEG image.",
                        False,
                    ),
                    (
                        5,
                        "Use image_gen.imagegen once to edit the last generated image "
                        "with num_last_images_to_include=1.",
                        True,
                    ),
                ]:
                    server.request(
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
                    image_evidence = wait_for_image_turn(
                        server, time.monotonic() + 120, require_history
                    )
                operation_count = 2
                turn_evidence = {
                    **image_evidence,
                    "history_edit": "completed",
                    "same_thread": True,
                }

            evidence = {
                "archive": archive.name,
                "archive_sha256": sha256(archive),
                "catalog": "release-bundled",
                "model": "grok-4.6",
                "multi_agent_version": "v2",
                "operation_count": operation_count,
                "provider": "grok",
                "reasoning_effort": "ultra",
                "scenario": scenario,
                "source_sha": source_sha,
                **turn_evidence,
                "story": f"grokex-{scenario}",
                "validation_run": run_id,
                "validator_sha": validator_sha,
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
