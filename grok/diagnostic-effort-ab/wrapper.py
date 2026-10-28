#!/usr/bin/env python3
"""Temporary experiment launcher: identical zero-retry settings in both arms."""
import hashlib
import os
from pathlib import Path
import re
import sys
import tomllib
from urllib.parse import urlsplit


def main():
    if sys.argv[1:] != ["app-server", "--strict-config", "--listen", "stdio://"]:
        raise ValueError("arguments")
    binary = Path(os.environ["GROK_EFFORT_REAL_BINARY"])
    expected = os.environ["GROK_EFFORT_REAL_SHA256"]
    if not binary.is_absolute() or not re.fullmatch(r"[0-9a-f]{64}", expected):
        raise ValueError("subject")
    with binary.open("rb") as stream:
        actual = hashlib.file_digest(stream, "sha256").hexdigest()
    if actual != expected:
        raise ValueError("digest")
    home = Path(os.environ["CODEX_HOME"])
    if not home.is_absolute():
        raise ValueError("home")
    config_path = home / "config.toml"
    text = config_path.read_text()
    config = tomllib.loads(text)
    provider = config["model_providers"]["grok"]
    endpoint = urlsplit(provider["base_url"])
    if (endpoint.scheme != "http" or endpoint.hostname != "127.0.0.1"
            or endpoint.port is None or endpoint.username is not None
            or endpoint.password is not None or endpoint.query or endpoint.fragment):
        raise ValueError("relay")
    if provider["base_url"] != os.environ["GROK_EFFORT_BASE_URL"]:
        raise ValueError("relay_identity")
    expected_model = {"A": "grok-4.6", "B": "grok-4.7"}[os.environ["GROK_EFFORT_ARM"]]
    if config["model"] != expected_model or config["model_provider"] != "grok":
        raise ValueError("model")
    if "request_max_retries" in provider or "stream_max_retries" in provider:
        raise ValueError("retry_already_set")
    marker = "[model_providers.grok]\n"
    if text.count(marker) != 1:
        raise ValueError("provider_section")
    patched = text.replace(marker, marker + "request_max_retries = 0\nstream_max_retries = 0\n", 1)
    check = tomllib.loads(patched)
    check["model_providers"]["grok"].pop("request_max_retries")
    check["model_providers"]["grok"].pop("stream_max_retries")
    if check != config:
        raise ValueError("config_delta")
    config_path.write_text(patched)
    os.execv(str(binary), [str(binary), *sys.argv[1:]])


if __name__ == "__main__":
    try:
        main()
    except Exception:
        # No exception bodies, paths, config, credentials, or wire traffic.
        sys.stderr.write("effort experiment launcher failed\n")
        sys.exit(1)
