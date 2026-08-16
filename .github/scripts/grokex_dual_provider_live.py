#!/usr/bin/env python3
"""Secret-safe live acceptance for Grokex provider behavior.

The same public app-server driver supports Grok-only CI and controlled
ChatGPT + Grok acceptance. It never prints model content, endpoint URLs,
credentials, configuration, or rollout data.
"""

from __future__ import annotations

import argparse
import http.client
import json
import os
import re
import selectors
import shutil
import socket
import ssl
import stat
import subprocess
import sys
import tempfile
import time
import tomllib
import urllib.error
import urllib.request
from collections import deque
from pathlib import Path
from typing import Any


class AcceptanceError(RuntimeError):
    pass


GROK_LIVE_MODEL = "grok-4.5"
HOSTED_TOOL_TYPES = ("web_search", "x_search", "image_generation")
X_SEARCH_NAMES = {
    "x_keyword_search",
    "x_semantic_search",
    "x_user_search",
    "x_thread_fetch",
}
HOSTED_PROBE_MAX_EVENT_BYTES = 64 * 1024 * 1024
HOSTED_PROBE_ERROR_BYTES = 64 * 1024
SAFE_ERROR_FIELD = re.compile(r"^[A-Za-z0-9_.-]{1,80}$")


def _resolve_hosted_tool_types(requested: list[str] | None) -> tuple[str, ...]:
    if requested is None:
        return HOSTED_TOOL_TYPES
    if not requested or len(requested) != len(set(requested)):
        raise AcceptanceError("hosted_tool_selection_invalid")
    selected = set(requested)
    return tuple(tool_type for tool_type in HOSTED_TOOL_TYPES if tool_type in selected)


def _toml_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def _write_evidence(path: Path, evidence: dict[str, Any]) -> None:
    encoded = (json.dumps(evidence, separators=(",", ":")) + "\n").encode()
    try:
        descriptor = os.open(
            path,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL,
            stat.S_IRUSR | stat.S_IWUSR,
        )
    except OSError as error:
        raise AcceptanceError("evidence_output_unavailable") from error
    with os.fdopen(descriptor, "wb") as output:
        output.write(encoded)


def _grok_profile(source: Path) -> tuple[str, dict[str, Any], str]:
    data = tomllib.loads(source.read_text(encoding="utf-8"))
    providers = data.get("model_providers")
    if not isinstance(providers, dict):
        raise AcceptanceError("configuration_has_no_provider_catalog")
    grok_profiles = [
        (candidate_id, profile)
        for candidate_id, profile in providers.items()
        if isinstance(profile, dict) and profile.get("provider_adapter") == "grok"
    ]
    if len(grok_profiles) == 1:
        provider_id, profile = grok_profiles[0]
    elif len(grok_profiles) == 0:
        provider_id = data.get("model_provider")
        profile = providers.get(provider_id) if isinstance(provider_id, str) else None
        if not isinstance(profile, dict):
            raise AcceptanceError("configuration_requires_one_grok_profile")
    else:
        raise AcceptanceError("configuration_requires_one_grok_profile")
    required_strings = ("base_url", "wire_api")
    if any(not isinstance(profile.get(key), str) for key in required_strings):
        raise AcceptanceError("grok_profile_is_incomplete")
    if profile["wire_api"] != "grok_responses":
        raise AcceptanceError("grok_profile_wire_api_is_invalid")
    if profile.get("provider_adapter") not in (None, "grok"):
        raise AcceptanceError("grok_profile_adapter_conflicts")
    credential_keys = [
        key
        for key in ("env_key", "experimental_bearer_token")
        if isinstance(profile.get(key), str) and profile[key]
    ]
    if len(credential_keys) != 1:
        raise AcceptanceError("grok_profile_requires_one_credential_path")
    if credential_keys == ["env_key"] and not os.environ.get(profile["env_key"]):
        raise AcceptanceError("grok_credential_environment_is_missing")
    return provider_id, profile, credential_keys[0]


def _write_isolated_config(source: Path, target: Path) -> str:
    provider_id, profile, credential_key = _grok_profile(source)

    lines = [
        'web_search = "live"',
        f'model_provider_registrations = ["openai", {_toml_string(provider_id)}]',
        "",
        "[features]",
        "multi_agent_v2 = true",
        "",
        f"[model_providers.{_toml_string(provider_id)}]",
        'name = "Grok"',
        f"base_url = {_toml_string(profile['base_url'])}",
        'provider_adapter = "grok"',
        'wire_api = "grok_responses"',
        f"x_search = {str(bool(profile.get('x_search', False))).lower()}",
        "requires_openai_auth = false",
    ]
    lines.append(f"{credential_key} = {_toml_string(profile[credential_key])}")
    if credential_key == "env_key" and isinstance(
        profile.get("env_key_instructions"), str
    ):
        lines.append(
            "env_key_instructions = "
            + _toml_string(profile["env_key_instructions"])
        )
    target.write_text("\n".join(lines) + "\n", encoding="utf-8")
    target.chmod(stat.S_IRUSR | stat.S_IWUSR)
    return provider_id


def _hosted_probe_request(model: str, tool_type: str) -> dict[str, Any]:
    prompts = {
        "web_search": "Use Web Search to find the official xAI home page.",
        "x_search": "Use X Search to find the official xAI account.",
        "image_generation": "Generate a simple blue circle on a white background.",
    }
    return {
        "model": model,
        "input": [{"role": "user", "content": prompts[tool_type]}],
        "tools": [{"type": tool_type}],
        "tool_choice": "required",
        "stream": True,
        "store": False,
    }


def _codex_live_user_agent(binary: Path) -> str:
    result = subprocess.run(
        [str(binary), "--version"],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        timeout=15,
    )
    fields = result.stdout.strip().split()
    version = fields[-1] if result.returncode == 0 and fields else ""
    if not SAFE_ERROR_FIELD.fullmatch(version):
        raise AcceptanceError("codex_binary_version_invalid")
    return f"codex_cli_rs/{version} (grokex_live_acceptance)"


def _hosted_probe_headers(token: str, user_agent: str) -> dict[str, str]:
    return {
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/json",
        "Accept": "text/event-stream",
        "User-Agent": user_agent,
        "originator": "codex_cli_rs",
    }


def _hosted_probe_error_classification(error: urllib.error.HTTPError) -> str:
    try:
        payload = json.loads(error.read(HOSTED_PROBE_ERROR_BYTES))
    except (json.JSONDecodeError, UnicodeDecodeError):
        return "unclassified"
    if not isinstance(payload, dict):
        return "unclassified"
    nested = payload.get("error")
    candidates = [
        nested.get("code") if isinstance(nested, dict) else None,
        payload.get("error_code"),
        payload.get("error_name"),
    ]
    return next(
        (
            candidate
            for candidate in candidates
            if isinstance(candidate, str) and SAFE_ERROR_FIELD.fullmatch(candidate)
        ),
        "unclassified",
    )


def _hosted_transport_error_classification(error: BaseException) -> str:
    reason = error.reason if isinstance(error, urllib.error.URLError) else error
    if isinstance(reason, socket.gaierror):
        return "dns"
    if isinstance(reason, (TimeoutError, socket.timeout)):
        return "timeout"
    if isinstance(reason, ssl.SSLError):
        return "tls"
    if isinstance(reason, http.client.RemoteDisconnected):
        return "remote_disconnect"
    if isinstance(reason, http.client.IncompleteRead):
        return "incomplete_read"
    if isinstance(
        reason,
        (ConnectionAbortedError, ConnectionResetError, BrokenPipeError),
    ):
        return "connection_closed"
    return "unclassified"


def _is_completed_hosted_item(event: dict[str, Any], tool_type: str) -> bool:
    if event.get("type") != "response.output_item.done":
        return False
    item = event.get("item")
    if not isinstance(item, dict) or item.get("status") != "completed":
        return False
    if tool_type == "web_search":
        return item.get("type") == "web_search_call"
    if tool_type == "x_search":
        return (
            item.get("type") == "custom_tool_call"
            and item.get("name") in X_SEARCH_NAMES
        )
    return (
        item.get("type") == "image_generation_call"
        and isinstance(item.get("result"), str)
        and bool(item["result"])
    )


def _probe_hosted_tool(
    endpoint: str,
    token: str,
    user_agent: str,
    model: str,
    tool_type: str,
) -> None:
    request = urllib.request.Request(
        endpoint,
        data=json.dumps(_hosted_probe_request(model, tool_type)).encode(),
        headers=_hosted_probe_headers(token, user_agent),
        method="POST",
    )
    completed = False
    event_data: list[bytes] = []
    event_bytes = 0
    try:
        with urllib.request.urlopen(request, timeout=300) as response:
            if response.status != 200:
                raise AcceptanceError(f"hosted_probe_http_status:{tool_type}")
            while True:
                line = response.readline(HOSTED_PROBE_MAX_EVENT_BYTES + 1)
                if not line:
                    break
                if len(line) > HOSTED_PROBE_MAX_EVENT_BYTES:
                    raise AcceptanceError(f"hosted_probe_event_too_large:{tool_type}")
                if line in (b"\n", b"\r\n"):
                    if event_data:
                        payload = b"\n".join(event_data)
                        if payload == b"[DONE]":
                            event_data = []
                            event_bytes = 0
                            continue
                        try:
                            event = json.loads(payload)
                        except (json.JSONDecodeError, UnicodeDecodeError) as error:
                            raise AcceptanceError(
                                f"hosted_probe_invalid_sse:{tool_type}"
                            ) from error
                        if isinstance(event, dict) and _is_completed_hosted_item(
                            event, tool_type
                        ):
                            completed = True
                        event_data = []
                        event_bytes = 0
                    continue
                if line.startswith(b"data:"):
                    value = line[5:].lstrip().rstrip(b"\r\n")
                    event_bytes += len(value)
                    if event_bytes > HOSTED_PROBE_MAX_EVENT_BYTES:
                        raise AcceptanceError(f"hosted_probe_event_too_large:{tool_type}")
                    event_data.append(value)
    except urllib.error.HTTPError as error:
        classification = _hosted_probe_error_classification(error)
        error.close()
        raise AcceptanceError(
            f"hosted_probe_http_status:{tool_type}:{error.code}:{classification}"
        ) from None
    except (urllib.error.URLError, http.client.HTTPException, OSError) as error:
        classification = _hosted_transport_error_classification(error)
        raise AcceptanceError(
            f"hosted_probe_transport:{tool_type}:{classification}"
        ) from None
    if not completed:
        raise AcceptanceError(f"hosted_probe_terminal_item_missing:{tool_type}")


def _run_gateway_hosted_live(
    source: Path,
    binary: Path,
    model: str,
    tool_types: tuple[str, ...] = HOSTED_TOOL_TYPES,
) -> None:
    _provider_id, profile, credential_key = _grok_profile(source)
    token = (
        os.environ[profile[credential_key]]
        if credential_key == "env_key"
        else profile[credential_key]
    )
    endpoint = profile["base_url"].rstrip("/") + "/responses"
    user_agent = _codex_live_user_agent(binary)
    for tool_type in tool_types:
        _probe_hosted_tool(endpoint, token, user_agent, model, tool_type)


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
            error_message = error.get("message") if isinstance(error, dict) else None
            classification = (
                ":provider_binding"
                if method == "turn/start"
                and isinstance(error_message, str)
                and "new thread" in error_message.lower()
                else ""
            )
            raise AcceptanceError(f"rpc_error:{method}:{code}{classification}")
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


def _assert_chatgpt_subscription_visible(server: AppServer) -> None:
    account = server.request("account/read", {"refreshToken": False})
    if not isinstance(account.get("account"), dict) or account["account"].get(
        "type"
    ) != "chatgpt":
        raise AcceptanceError("chatgpt_subscription_not_visible")
    if account.get("requiresOpenaiAuth") is not False:
        raise AcceptanceError("requires_openai_auth_not_stable")
    auth_status = server.request(
        "getAuthStatus", {"includeToken": False, "refreshToken": False}
    )
    if auth_status.get("authMethod") != "chatgpt":
        raise AcceptanceError("chatgpt_auth_method_not_visible")
    if auth_status.get("requiresOpenaiAuth") is not False:
        raise AcceptanceError("requires_openai_auth_not_stable")


def _models(server: AppServer) -> dict[str, str]:
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

    def choose(
        *,
        required_error: str | None,
        exact_model: str | None = None,
        display_prefix: str | None = None,
    ) -> str | None:
        matches = [
            model
            for model in models
            if isinstance(model.get("model"), str)
            and (exact_model is None or model["model"] == exact_model)
            and (
                display_prefix is None
                or (
                    isinstance(model.get("displayName"), str)
                    and model["displayName"].startswith(display_prefix)
                )
            )
        ]
        if not matches:
            if required_error is not None:
                raise AcceptanceError(required_error)
            return None
        selected = next((model for model in matches if model.get("isDefault")), matches[0])
        return selected["model"]

    grok = choose(
        required_error="grok_model_catalog_incomplete",
        exact_model=GROK_LIVE_MODEL,
    )
    assert grok is not None
    selected = {"grok": grok}
    chatgpt = choose(required_error=None, display_prefix="ChatGPT · ")
    if chatgpt is not None:
        selected["openai"] = chatgpt
    return selected


def _start_thread(server: AppServer, model: str, workspace: Path) -> tuple[str, str]:
    result = server.request(
        "thread/start",
        {
            "model": model,
            "cwd": str(workspace),
            "approvalPolicy": "never",
            "environments": [
                {
                    "environmentId": "local",
                    "cwd": str(workspace),
                }
            ],
        },
    )
    thread = result.get("thread")
    if not isinstance(thread, dict) or not isinstance(thread.get("id"), str):
        raise AcceptanceError("thread_start_invalid")
    provider = result.get("modelProvider")
    if not isinstance(provider, str):
        raise AcceptanceError("thread_provider_missing")
    return thread["id"], provider


def _start_turn(server: AppServer, thread_id: str, prompt: str) -> str:
    result = server.request(
        "turn/start",
        {
            "threadId": thread_id,
            "input": [
                {
                    "type": "text",
                    "text": prompt,
                    "text_elements": [],
                }
            ],
        },
    )
    turn = result.get("turn")
    if not isinstance(turn, dict) or not isinstance(turn.get("id"), str):
        raise AcceptanceError("turn_start_invalid")
    return turn["id"]


def _wait_turn(
    server: AppServer,
    thread_id: str,
    turn_id: str,
    required_item_types: tuple[str, ...] = ("agentMessage",),
) -> dict[str, Any]:
    params = server.wait_notification(
        "turn/completed",
        lambda value: value.get("threadId") == thread_id
        and isinstance(value.get("turn"), dict)
        and value["turn"].get("id") == turn_id,
    )
    turn = params.get("turn")
    if not isinstance(turn, dict) or turn.get("status") != "completed":
        raise AcceptanceError("turn_did_not_complete")
    items = turn.get("items")
    if not isinstance(items, list):
        raise AcceptanceError("turn_items_missing")
    for required_type in required_item_types:
        if not any(
            isinstance(item, dict)
            and item.get("type") == required_type
            and (
                required_type != "commandExecution"
                or (
                    item.get("source") == "agent"
                    and item.get("status") == "completed"
                )
            )
            for item in items
        ):
            present_types = sorted(
                {
                    item_type
                    for item in items
                    if isinstance(item, dict)
                    and isinstance((item_type := item.get("type")), str)
                    and SAFE_ERROR_FIELD.fullmatch(item_type)
                }
            )
            present = ",".join(present_types) if present_types else "none"
            raise AcceptanceError(
                f"turn_item_evidence_missing:{required_type}:present={present}"
            )
    return turn


def _start_message_turn(server: AppServer, thread_id: str) -> str:
    return _start_turn(
        server,
        thread_id,
        "Reply with a brief confirmation. Do not call any tool.",
    )


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
    completed = server.wait_notification(
        "item/completed",
        lambda value: value.get("threadId") == thread_id
        and isinstance(value.get("item"), dict)
        and value["item"].get("type") == "contextCompaction",
    )
    turn_id = completed.get("turnId")
    if not isinstance(turn_id, str):
        raise AcceptanceError("compaction_turn_id_missing")
    params = server.wait_notification(
        "turn/completed",
        lambda value: value.get("threadId") == thread_id
        and isinstance(value.get("turn"), dict)
        and value["turn"].get("id") == turn_id,
    )
    turn = params.get("turn")
    if not isinstance(turn, dict) or turn.get("status") != "completed":
        raise AcceptanceError("compaction_turn_did_not_complete")


def _spawn_child(server: AppServer, thread_id: str, provider: str) -> str:
    existing_children = server.request(
        "thread/list", {"limit": 20, "parentThreadId": thread_id}
    ).get("data")
    if not isinstance(existing_children, list):
        raise AcceptanceError("subagent_provider_binding_missing")
    existing_ids = {
        child["id"]
        for child in existing_children
        if isinstance(child, dict) and isinstance(child.get("id"), str)
    }
    turn_id = _start_turn(
        server,
        thread_id,
        (
            "Use spawn_agent to create one reviewer. Ask it to review this request, "
            "wait for it to complete, and then reply briefly."
        ),
    )
    _wait_turn(server, thread_id, turn_id)
    deadline = time.monotonic() + 300
    while True:
        children = server.request(
            "thread/list", {"limit": 20, "parentThreadId": thread_id}
        ).get("data")
        if not isinstance(children, list):
            raise AcceptanceError("subagent_provider_binding_missing")
        matching_children = [
            child
            for child in children
            if isinstance(child, dict)
            and child.get("id") not in existing_ids
            and child.get("modelProvider") == provider
            and isinstance(child.get("id"), str)
        ]
        if matching_children:
            child = matching_children[0]
            status = child.get("status")
            if isinstance(status, dict) and status.get("type") == "idle":
                read = server.request(
                    "thread/read", {"threadId": child["id"], "includeTurns": True}
                )
                persisted = read.get("thread")
                turns = persisted.get("turns") if isinstance(persisted, dict) else None
                if not isinstance(turns, list) or not any(
                    isinstance(turn, dict)
                    and turn.get("status") == "completed"
                    and isinstance(turn.get("items"), list)
                    and any(
                        isinstance(item, dict)
                        and item.get("type") == "agentMessage"
                        for item in turn["items"]
                    )
                    for turn in turns
                ):
                    raise AcceptanceError("subagent_completed_turn_missing")
                return child["id"]
        if time.monotonic() >= deadline:
            raise AcceptanceError("subagent_did_not_complete")
        time.sleep(1)


def _interactive_thread_list_has_bindings(
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


def _assert_cross_provider_model_is_rejected(
    server: AppServer, thread_id: str, foreign_model: str
) -> None:
    try:
        server.request(
            "turn/start",
            {
                "threadId": thread_id,
                "model": foreign_model,
                "input": [
                    {
                        "type": "text",
                        "text": "This request must be rejected before provider egress.",
                        "text_elements": [],
                    }
                ],
            },
        )
    except AcceptanceError as error:
        if str(error) == "rpc_error:turn/start:-32600:provider_binding":
            return
        raise
    raise AcceptanceError("cross_provider_model_was_not_rejected")


def _assert_openai_does_not_target_grok(config: Path) -> None:
    data = tomllib.loads(config.read_text(encoding="utf-8"))
    if "model" in data or "model_provider" in data:
        raise AcceptanceError("isolated_config_has_top_level_provider_override")
    openai = data.get("model_providers", {}).get("openai")
    if isinstance(openai, dict) and (
        openai.get("provider_adapter") == "grok"
        or openai.get("wire_api") == "grok_responses"
    ):
        raise AcceptanceError("openai_profile_uses_grok_adapter_or_dialect")


def _start_grok_only(
    server: AppServer,
    grok_model: str,
    grok_provider: str,
    workspace: Path,
) -> str:
    thread_id, resolved_provider = _start_thread(server, grok_model, workspace)
    if resolved_provider != grok_provider:
        raise AcceptanceError("grok_model_owner_mismatch")
    turn_id = _start_message_turn(server, thread_id)
    _wait_turn(server, thread_id, turn_id)
    return thread_id


def _resume_grok_only(
    server: AppServer,
    thread_id: str,
    grok_model: str,
    grok_provider: str,
) -> dict[str, Any]:
    _resume(server, thread_id, grok_provider)
    turn_id = _start_message_turn(server, thread_id)
    _wait_turn(server, thread_id, turn_id)

    fork_id = _fork(server, thread_id, grok_provider)
    fork_turn_id = _start_message_turn(server, fork_id)
    _wait_turn(server, fork_id, fork_turn_id)

    _compact(server, thread_id)
    child_id = _spawn_child(server, thread_id, grok_provider)

    _interactive_thread_list_has_bindings(
        server,
        {
            thread_id: grok_provider,
            fork_id: grok_provider,
        },
    )
    return {
        "provider": grok_provider,
        "model": grok_model,
        "thread_ids": [
            thread_id,
            fork_id,
            child_id,
        ],
    }


def run(args: argparse.Namespace) -> None:
    binary = args.codex_bin.resolve()
    auth_source = args.chatgpt_auth.resolve() if args.chatgpt_auth else None
    grok_source = args.grok_config.resolve()
    workspace = args.workspace.resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise AcceptanceError("codex_binary_unavailable")
    if not args.grok_only and (auth_source is None or not auth_source.is_file()):
        raise AcceptanceError("chatgpt_auth_unavailable")
    if not grok_source.is_file():
        raise AcceptanceError("grok_config_unavailable")
    hosted_tool_types = _resolve_hosted_tool_types(args.hosted_tools)
    if args.evidence_output is not None and hosted_tool_types != HOSTED_TOOL_TYPES:
        raise AcceptanceError("partial_hosted_evidence_output_not_supported")
    workspace.mkdir(parents=True, exist_ok=True)

    started_at = time.time()
    with tempfile.TemporaryDirectory(prefix="grokex-dual-provider-") as temp:
        codex_home = Path(temp)
        codex_home.chmod(stat.S_IRWXU)
        if auth_source is not None:
            shutil.copyfile(auth_source, codex_home / "auth.json")
            (codex_home / "auth.json").chmod(stat.S_IRUSR | stat.S_IWUSR)
        grok_provider = _write_isolated_config(grok_source, codex_home / "config.toml")
        _assert_openai_does_not_target_grok(codex_home / "config.toml")
        _run_gateway_hosted_live(
            grok_source,
            binary,
            GROK_LIVE_MODEL,
            hosted_tool_types,
        )

        server = AppServer(binary, codex_home, workspace)
        try:
            _initialize(server)
            models = _models(server)
            grok_model = models["grok"]
            if args.grok_only:
                grok_thread = _start_grok_only(
                    server, grok_model, grok_provider, workspace
                )
            else:
                _assert_chatgpt_subscription_visible(server)
                openai_model = models.get("openai")
                if openai_model is None:
                    raise AcceptanceError("chatgpt_model_catalog_incomplete")
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

                _assert_cross_provider_model_is_rejected(
                    server, openai_thread, grok_model
                )
                _assert_cross_provider_model_is_rejected(
                    server, grok_thread, openai_model
                )

                openai_turn = _start_message_turn(server, openai_thread)
                grok_turn = _start_message_turn(server, grok_thread)
                _wait_turn(server, openai_thread, openai_turn)
                _wait_turn(server, grok_thread, grok_turn)
        finally:
            server.close()

        server = AppServer(binary, codex_home, workspace)
        try:
            _initialize(server)
            if args.grok_only:
                grok_evidence = _resume_grok_only(
                    server,
                    grok_thread,
                    grok_model,
                    grok_provider,
                )
                openai_evidence = None
            else:
                _assert_chatgpt_subscription_visible(server)
                _resume(server, openai_thread, openai_provider)
                _resume(server, grok_thread, resolved_grok_provider)
                openai_turn = _start_message_turn(server, openai_thread)
                grok_turn = _start_message_turn(server, grok_thread)
                _wait_turn(server, openai_thread, openai_turn)
                _wait_turn(server, grok_thread, grok_turn)
                openai_fork = _fork(server, openai_thread, openai_provider)
                grok_fork = _fork(server, grok_thread, resolved_grok_provider)
                _interactive_thread_list_has_bindings(
                    server,
                    {
                        openai_thread: openai_provider,
                        grok_thread: resolved_grok_provider,
                        openai_fork: openai_provider,
                        grok_fork: resolved_grok_provider,
                    },
                )
                openai_fork_turn = _start_message_turn(server, openai_fork)
                grok_fork_turn = _start_message_turn(server, grok_fork)
                _wait_turn(server, openai_fork, openai_fork_turn)
                _wait_turn(server, grok_fork, grok_fork_turn)
                _compact(server, openai_thread)
                _compact(server, grok_thread)
                openai_child = _spawn_child(server, openai_thread, openai_provider)
                grok_child = _spawn_child(
                    server, grok_thread, resolved_grok_provider
                )
        finally:
            server.close()

        if not args.grok_only:
            openai_evidence = {
                "provider": openai_provider,
                "model": openai_model,
                "thread_ids": [openai_thread, openai_fork, openai_child],
            }
            grok_evidence = {
                "provider": resolved_grok_provider,
                "model": grok_model,
                "thread_ids": [grok_thread, grok_fork, grok_child],
            }
    _finish(args, started_at, openai_evidence, grok_evidence, hosted_tool_types)


def _finish(
    args: argparse.Namespace,
    started_at: float,
    openai: dict[str, Any] | None,
    grok: dict[str, Any],
    hosted_tool_types: tuple[str, ...] = HOSTED_TOOL_TYPES,
) -> None:
    if args.evidence_output is not None:
        evidence = {
            "schema": "grokex_dual_provider_live/v2",
            "started_at_unix": started_at,
            "completed_at_unix": time.time(),
            "openai": None
            if openai is None
            else {
                "provider_binding": "passed",
                "thread_count": len(openai["thread_ids"]),
            },
            "grok": {
                "provider_binding": "passed",
                "thread_count": len(grok["thread_ids"]),
            },
            "chatgpt_application_auth_contract": "not_run"
            if openai is None
            else "passed",
            "grok_hosted_gateway_live": "passed",
        }
        _write_evidence(args.evidence_output, evidence)

    print("live_mode=" + ("dual_provider" if openai is not None else "grok_only"))
    if openai is not None:
        print("chatgpt_provider_binding=passed")
        print("chatgpt_application_auth_contract=passed")
    print("grok_provider_binding=passed")
    if hosted_tool_types == HOSTED_TOOL_TYPES:
        print("grok_hosted_gateway_live=passed")
    else:
        for tool_type in hosted_tool_types:
            print(f"grok_hosted_{tool_type}=passed")
        print("grok_hosted_gateway_live=partial")
    print("mini_accounting_gate=requires_operator_usage_evidence")
    print("isolated_runtime_home_cleanup=passed")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--codex-bin", type=Path, required=True)
    parser.add_argument("--chatgpt-auth", type=Path)
    parser.add_argument("--grok-config", type=Path, required=True)
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--evidence-output", type=Path)
    parser.add_argument(
        "--hosted-tool",
        dest="hosted_tools",
        action="append",
        choices=HOSTED_TOOL_TYPES,
        help="Run only this hosted-tool contract; repeat to select more than one.",
    )
    parser.add_argument("--grok-only", action="store_true")
    return parser.parse_args()


if __name__ == "__main__":
    try:
        run(parse_args())
    except AcceptanceError as error:
        print(f"dual_provider_live=failed:{error}", file=sys.stderr)
        raise SystemExit(1) from None
