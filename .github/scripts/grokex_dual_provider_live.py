#!/usr/bin/env python3
"""Secret-safe live acceptance for one ChatGPT + Grok Codex Home.

This driver uses only the public app-server protocol. It never prints model
content, endpoint URLs, credentials, configuration, or rollout data.
"""

from __future__ import annotations

import argparse
import json
import os
import selectors
import shutil
import stat
import subprocess
import sys
import tempfile
import time
import tomllib
from collections import deque
from pathlib import Path
from typing import Any


class AcceptanceError(RuntimeError):
    pass


def _toml_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def _write_isolated_config(source: Path, target: Path) -> str:
    data = tomllib.loads(source.read_text(encoding="utf-8"))
    providers = data.get("model_providers")
    if not isinstance(providers, dict):
        raise AcceptanceError("configuration_has_no_provider_catalog")
    grok_profiles = [
        (provider_id, profile)
        for provider_id, profile in providers.items()
        if isinstance(profile, dict) and profile.get("wire_api") == "grok_responses"
    ]
    if len(grok_profiles) != 1:
        raise AcceptanceError("configuration_requires_one_grok_profile")
    provider_id, profile = grok_profiles[0]
    required_strings = ("base_url", "wire_api")
    if any(not isinstance(profile.get(key), str) for key in required_strings):
        raise AcceptanceError("grok_profile_is_incomplete")
    credential_keys = [
        key
        for key in ("env_key", "experimental_bearer_token")
        if isinstance(profile.get(key), str) and profile[key]
    ]
    if len(credential_keys) != 1:
        raise AcceptanceError("grok_profile_requires_one_credential_path")
    if credential_keys == ["env_key"] and not os.environ.get(profile["env_key"]):
        raise AcceptanceError("grok_credential_environment_is_missing")

    lines = [
        'web_search = "live"',
        "",
        "[features]",
        "multi_agent_v2 = true",
        "",
        f"[model_providers.{_toml_string(provider_id)}]",
        'name = "Grok"',
        f"base_url = {_toml_string(profile['base_url'])}",
        'wire_api = "grok_responses"',
        f"x_search = {str(bool(profile.get('x_search', False))).lower()}",
        "requires_openai_auth = false",
    ]
    for key in credential_keys:
        lines.append(f"{key} = {_toml_string(profile[key])}")
    if credential_keys == ["env_key"] and isinstance(
        profile.get("env_key_instructions"), str
    ):
        lines.append(
            "env_key_instructions = "
            + _toml_string(profile["env_key_instructions"])
        )
    target.write_text("\n".join(lines) + "\n", encoding="utf-8")
    target.chmod(stat.S_IRUSR | stat.S_IWUSR)
    return provider_id


class AppServer:
    def __init__(self, binary: Path, codex_home: Path, workspace: Path) -> None:
        env = os.environ.copy()
        env["CODEX_HOME"] = str(codex_home)
        self._process = subprocess.Popen(
            [str(binary), "app-server", "--stdio"],
            cwd=workspace,
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        if self._process.stdin is None or self._process.stdout is None:
            raise AcceptanceError("app_server_stdio_unavailable")
        self._stdin = self._process.stdin
        self._stdout = self._process.stdout
        self._selector = selectors.DefaultSelector()
        self._selector.register(self._stdout, selectors.EVENT_READ)
        self._next_id = 1
        self._pending: deque[dict[str, Any]] = deque()
        self._responses: dict[int, dict[str, Any]] = {}
        self._stdout_buffer = bytearray()

    def close(self) -> None:
        try:
            self._stdin.close()
        except OSError:
            pass
        try:
            self._process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            self._process.terminate()
            try:
                self._process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self._process.kill()
                self._process.wait(timeout=5)
        self._selector.close()
        self._stdout.close()

    def _send(self, message: dict[str, Any]) -> None:
        encoded = (json.dumps(message, separators=(",", ":")) + "\n").encode()
        self._stdin.write(encoded)
        self._stdin.flush()

    def notify(self, method: str, params: dict[str, Any] | None = None) -> None:
        message: dict[str, Any] = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            message["params"] = params
        self._send(message)

    def request(
        self, method: str, params: dict[str, Any], timeout: float = 90
    ) -> dict[str, Any]:
        request_id = self._next_id
        self._next_id += 1
        self._send(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "method": method,
                "params": params,
            }
        )
        deadline = time.monotonic() + timeout
        while True:
            buffered = self._responses.pop(request_id, None)
            if buffered is not None:
                return self._response_result(method, buffered)
            message = self._read(deadline)
            response_id = message.get("id")
            if response_id == request_id:
                return self._response_result(method, message)
            if isinstance(response_id, int):
                self._responses[response_id] = message
                continue
            self._pending.append(message)

    @staticmethod
    def _response_result(method: str, message: dict[str, Any]) -> dict[str, Any]:
        if "error" in message:
            error = message.get("error")
            code = error.get("code", "unknown") if isinstance(error, dict) else "unknown"
            raise AcceptanceError(f"rpc_error:{method}:{code}")
        result = message.get("result")
        if not isinstance(result, dict):
            raise AcceptanceError(f"rpc_result_invalid:{method}")
        return result

    def _read(self, deadline: float) -> dict[str, Any]:
        while b"\n" not in self._stdout_buffer:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise AcceptanceError("app_server_timeout")
            if not self._selector.select(remaining):
                raise AcceptanceError("app_server_timeout")
            chunk = os.read(self._stdout.fileno(), 64 * 1024)
            if not chunk:
                raise AcceptanceError("app_server_exited")
            self._stdout_buffer.extend(chunk)
        line, _, remainder = self._stdout_buffer.partition(b"\n")
        self._stdout_buffer = bytearray(remainder)
        try:
            message = json.loads(line.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise AcceptanceError("app_server_non_json_output") from error
        if not isinstance(message, dict):
            raise AcceptanceError("app_server_invalid_message")
        if "method" in message and "id" in message:
            raise AcceptanceError("unexpected_server_request")
        return message

    def wait_notification(
        self,
        method: str,
        predicate: Any,
        timeout: float = 300,
    ) -> dict[str, Any]:
        deadline = time.monotonic() + timeout
        while True:
            retained: deque[dict[str, Any]] = deque()
            matched: dict[str, Any] | None = None
            while self._pending:
                message = self._pending.popleft()
                if matched is None and message.get("method") == method:
                    params = message.get("params")
                    if isinstance(params, dict) and predicate(params):
                        matched = params
                        continue
                retained.append(message)
            self._pending = retained
            if matched is not None:
                return matched
            message = self._read(deadline)
            response_id = message.get("id")
            if isinstance(response_id, int):
                self._responses[response_id] = message
                continue
            if message.get("method") == method:
                params = message.get("params")
                if isinstance(params, dict) and predicate(params):
                    return params
            self._pending.append(message)


def _initialize(server: AppServer) -> None:
    server.request(
        "initialize",
        {
            "clientInfo": {
                "name": "grokex_live_acceptance",
                "title": "Grokex Live Acceptance",
                "version": "1.0.0",
            },
            "capabilities": {"experimentalApi": True},
        },
    )
    server.notify("initialized")


def _models(server: AppServer) -> tuple[str, str]:
    models: list[dict[str, Any]] = []
    cursor: str | None = None
    while True:
        params: dict[str, Any] = {"limit": 100}
        if cursor is not None:
            params["cursor"] = cursor
        page = server.request("model/list", params)
        data = page.get("data")
        if not isinstance(data, list):
            raise AcceptanceError("model_catalog_invalid")
        models.extend(model for model in data if isinstance(model, dict))
        cursor = page.get("nextCursor")
        if not isinstance(cursor, str) or not cursor:
            break

    def choose(prefix: str) -> str:
        matches = [
            model
            for model in models
            if isinstance(model.get("displayName"), str)
            and model["displayName"].startswith(prefix)
            and isinstance(model.get("model"), str)
        ]
        if not matches:
            raise AcceptanceError("unified_model_catalog_incomplete")
        selected = next((model for model in matches if model.get("isDefault")), matches[0])
        return selected["model"]

    return choose("ChatGPT · "), choose("Grok · ")


def _start_thread(server: AppServer, model: str, workspace: Path) -> tuple[str, str]:
    result = server.request(
        "thread/start",
        {
            "model": model,
            "cwd": str(workspace),
            "approvalPolicy": "never",
        },
    )
    thread = result.get("thread")
    if not isinstance(thread, dict) or not isinstance(thread.get("id"), str):
        raise AcceptanceError("thread_start_invalid")
    provider = result.get("modelProvider")
    if not isinstance(provider, str):
        raise AcceptanceError("thread_provider_missing")
    return thread["id"], provider


def _start_turn(server: AppServer, thread_id: str, marker: str) -> None:
    server.request(
        "turn/start",
        {
            "threadId": thread_id,
            "input": [
                {
                    "type": "text",
                    "text": f"Reply with exactly {marker}. Do not call any tool.",
                    "text_elements": [],
                }
            ],
        },
    )


def _wait_turn(server: AppServer, thread_id: str) -> None:
    params = server.wait_notification(
        "turn/completed", lambda value: value.get("threadId") == thread_id
    )
    turn = params.get("turn")
    if not isinstance(turn, dict) or turn.get("status") != "completed":
        raise AcceptanceError("turn_did_not_complete")


def _resume(server: AppServer, thread_id: str, provider: str) -> None:
    result = server.request("thread/resume", {"threadId": thread_id})
    if result.get("modelProvider") != provider:
        raise AcceptanceError("resume_changed_provider")


def _fork(server: AppServer, thread_id: str, provider: str) -> str:
    result = server.request("thread/fork", {"threadId": thread_id})
    if result.get("modelProvider") != provider:
        raise AcceptanceError("fork_changed_provider")
    thread = result.get("thread")
    if not isinstance(thread, dict) or not isinstance(thread.get("id"), str):
        raise AcceptanceError("fork_result_invalid")
    return thread["id"]


def _compact(server: AppServer, thread_id: str) -> None:
    server.request("thread/compact/start", {"threadId": thread_id})
    server.wait_notification(
        "item/completed",
        lambda value: value.get("threadId") == thread_id
        and isinstance(value.get("item"), dict)
        and value["item"].get("type") == "contextCompaction",
    )


def _spawn_child(server: AppServer, thread_id: str, provider: str, marker: str) -> None:
    server.request(
        "turn/start",
        {
            "threadId": thread_id,
            "input": [
                {
                    "type": "text",
                    "text": (
                        "Use spawn_agent to create one reviewer. Tell it to reply with "
                        f"exactly {marker}. Wait for it, then return exactly that marker."
                    ),
                    "text_elements": [],
                }
            ],
        },
    )
    _wait_turn(server, thread_id)
    children = server.request(
        "thread/list", {"limit": 20, "parentThreadId": thread_id}
    ).get("data")
    if not isinstance(children, list) or not any(
        isinstance(child, dict) and child.get("modelProvider") == provider
        for child in children
    ):
        raise AcceptanceError("subagent_provider_binding_missing")


def _thread_list_has_bindings(
    server: AppServer, expected: dict[str, str]
) -> None:
    threads = server.request("thread/list", {"limit": 100}).get("data")
    if not isinstance(threads, list):
        raise AcceptanceError("thread_list_invalid")
    actual = {
        thread.get("id"): thread.get("modelProvider")
        for thread in threads
        if isinstance(thread, dict)
    }
    if any(actual.get(thread_id) != provider for thread_id, provider in expected.items()):
        raise AcceptanceError("thread_list_provider_binding_missing")


def _assert_openai_does_not_target_grok(config: Path) -> None:
    data = tomllib.loads(config.read_text(encoding="utf-8"))
    if "model" in data or "model_provider" in data:
        raise AcceptanceError("isolated_config_has_top_level_provider_override")
    openai = data.get("model_providers", {}).get("openai")
    if isinstance(openai, dict) and openai.get("wire_api") == "grok_responses":
        raise AcceptanceError("openai_profile_uses_grok_dialect")


def run(args: argparse.Namespace) -> None:
    binary = args.codex_bin.resolve()
    auth_source = args.chatgpt_auth.resolve()
    grok_source = args.grok_config.resolve()
    workspace = args.workspace.resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise AcceptanceError("codex_binary_unavailable")
    if not auth_source.is_file():
        raise AcceptanceError("chatgpt_auth_unavailable")
    if not grok_source.is_file():
        raise AcceptanceError("grok_config_unavailable")
    workspace.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="grokex-dual-provider-") as temp:
        codex_home = Path(temp)
        codex_home.chmod(stat.S_IRWXU)
        shutil.copyfile(auth_source, codex_home / "auth.json")
        (codex_home / "auth.json").chmod(stat.S_IRUSR | stat.S_IWUSR)
        grok_provider = _write_isolated_config(grok_source, codex_home / "config.toml")
        _assert_openai_does_not_target_grok(codex_home / "config.toml")

        server = AppServer(binary, codex_home, workspace)
        try:
            _initialize(server)
            account = server.request("account/read", {"refreshToken": False})
            if (
                not isinstance(account.get("account"), dict)
                or account["account"].get("type") != "chatgpt"
                or account.get("requiresOpenaiAuth") is not True
            ):
                raise AcceptanceError("chatgpt_subscription_not_visible")
            openai_model, grok_model = _models(server)
            openai_thread, openai_provider = _start_thread(
                server, openai_model, workspace
            )
            grok_thread, resolved_grok_provider = _start_thread(
                server, grok_model, workspace
            )
            if openai_provider == resolved_grok_provider:
                raise AcceptanceError("provider_catalog_not_federated")
            if resolved_grok_provider != grok_provider:
                raise AcceptanceError("grok_model_owner_mismatch")

            _start_turn(server, openai_thread, "GROKEX_OPENAI_CONCURRENT_OK")
            _start_turn(server, grok_thread, "GROKEX_GROK_CONCURRENT_OK")
            _wait_turn(server, openai_thread)
            _wait_turn(server, grok_thread)
        finally:
            server.close()

        server = AppServer(binary, codex_home, workspace)
        try:
            _initialize(server)
            _resume(server, openai_thread, openai_provider)
            _resume(server, grok_thread, resolved_grok_provider)
            openai_fork = _fork(server, openai_thread, openai_provider)
            grok_fork = _fork(server, grok_thread, resolved_grok_provider)
            _thread_list_has_bindings(
                server,
                {
                    openai_thread: openai_provider,
                    grok_thread: resolved_grok_provider,
                    openai_fork: openai_provider,
                    grok_fork: resolved_grok_provider,
                },
            )
            _compact(server, openai_thread)
            _compact(server, grok_thread)
            _spawn_child(
                server, openai_thread, openai_provider, "GROKEX_OPENAI_CHILD_OK"
            )
            _spawn_child(
                server,
                grok_thread,
                resolved_grok_provider,
                "GROKEX_GROK_CHILD_OK",
            )
        finally:
            server.close()

    print("dual_provider_live=passed")
    print("chatgpt_provider_binding=passed")
    print("grok_provider_binding=passed")
    print("mini_accounting_gate=requires_operator_usage_evidence")
    print("credential_and_home_cleanup=passed")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--codex-bin", type=Path, required=True)
    parser.add_argument("--chatgpt-auth", type=Path, required=True)
    parser.add_argument("--grok-config", type=Path, required=True)
    parser.add_argument("--workspace", type=Path, required=True)
    return parser.parse_args()


if __name__ == "__main__":
    try:
        run(parse_args())
    except AcceptanceError as error:
        print(f"dual_provider_live=failed:{error}", file=sys.stderr)
        raise SystemExit(1) from None
