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
            [str(binary), "app-server", "--strict-config", "--listen", "off"],
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


def run_smoke(
    archive: Path,
    config: Path,
    evidence_path: Path,
    source_sha: str,
    run_id: str,
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

            thread_response = server.request(
                3,
                "thread/start",
                {
                    "cwd": str(workspace),
                    "ephemeral": True,
                    "model": "grok-4.6",
                    "modelProvider": "grok",
                },
            )
            thread = thread_response.get("thread")
            if not isinstance(thread, dict) or thread_response.get("modelProvider") != "grok":
                raise SystemExit("thread/start did not bind Provider grok")
            thread_id = thread.get("id")
            if not isinstance(thread_id, str) or not thread_id:
                raise SystemExit("thread/start returned no thread identity")

            operation_count = 1
            server.request(
                4,
                "turn/start",
                {
                    "input": [{"text": "Reply with OK.", "textElements": [], "type": "text"}],
                    "threadId": thread_id,
                },
            )

            agent_message_completed = False
            status = None
            deadline = time.monotonic() + 120
            while time.monotonic() < deadline:
                message = server.next_message(deadline, "the single Grok Turn")
                method = message.get("method")
                params = message.get("params")
                if not isinstance(params, dict):
                    continue
                if method == "item/completed":
                    item = params.get("item")
                    if isinstance(item, dict) and item.get("type") == "agentMessage":
                        agent_message_completed = True
                elif method == "turn/completed":
                    turn = params.get("turn")
                    status = turn.get("status") if isinstance(turn, dict) else None
                    break
            if status != "completed" or not agent_message_completed:
                raise SystemExit("the single Grok Turn did not complete with an agent message")

            evidence = {
                "archive": archive.name,
                "archive_sha256": sha256(archive),
                "catalog": "release-bundled",
                "model": "grok-4.6",
                "multi_agent_version": "v2",
                "operation_count": operation_count,
                "provider": "grok",
                "reasoning_effort": "ultra",
                "source_sha": source_sha,
                "status": status,
                "story": "grokex-provider-profile-startup",
                "validation_run": run_id,
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
    parser.add_argument("--run-id", required=True)
    args = parser.parse_args()
    run_smoke(args.archive, args.config, args.evidence, args.source_sha, args.run_id)


if __name__ == "__main__":
    main()
