#!/usr/bin/env python3
"""Run one bounded, secret-safe Grok Turn through a packaged App Server."""

from __future__ import annotations

import argparse
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
SCENARIOS = (BASIC_SCENARIO, CONTINUATION_SCENARIO)
BASIC_EXPECTED_AGENT_REPLY = "GROKEX_BASIC_RESPONSE_OK"
TOOL_NAME = "grokex_live_probe"
TOOL_OUTPUT_MARKER = "GROKEX_LIVE_TOOL_OK"
EXPECTED_AGENT_REPLY = "GROKEX_LIVE_RESPONSE_OK"


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


def wait_for_basic_turn(server: AppServer, deadline: float) -> dict[str, str]:
    agent_reply = None
    status = None

    while time.monotonic() < deadline:
        message = server.next_message(deadline, "the single basic Grok Turn")
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
            "the single basic Grok Turn did not complete "
            f"(status={safe_status}, "
            f"agent_reply_seen={str(isinstance(agent_reply, str)).lower()})"
        )
    if not isinstance(agent_reply, str) or agent_reply.strip() != BASIC_EXPECTED_AGENT_REPLY:
        raise SystemExit("the basic Grok Turn did not return the expected semantic reply")
    return {
        "response_assertion": "exact_match",
        "status": status,
    }


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
        "reasoning_replay": "completed",
        "response_assertion": "exact_match",
        "status": status,
        "tool_continuation": "completed",
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
                wait_for_turn = wait_for_basic_turn
            else:
                prompt = (
                    f"Call {TOOL_NAME} exactly once. Use its result, then reply "
                    f"with exactly {EXPECTED_AGENT_REPLY} and no other text."
                )
                wait_for_turn = wait_for_verified_turn

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
            turn_evidence = wait_for_turn(server, time.monotonic() + 120)

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
