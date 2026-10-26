#!/usr/bin/env python3
"""Small workflow evidence finalizer; no backend operations or raw log upload."""
import argparse
import json
import os
from pathlib import Path
import re


def safe_stderr(data: bytes, secret: str) -> dict:
    truncated = len(data) > 65536
    prefix = data[:65536]
    # Never retain a partial final line: a credential may straddle the bound.
    if truncated:
        prefix = prefix[:prefix.rfind(b"\n") + 1]
    text = prefix.decode("utf-8", errors="replace")
    if secret:
        text = text.replace(secret, "[REDACTED]")
    lines = []
    sensitive = re.compile(r"(?i)(authorization|api[_-]?key|password|access[_-]?token|refresh[_-]?token|cookie|secret)\s*[\"']?\s*[:=]")
    for line in text.splitlines(keepends=True):
        if sensitive.search(line):
            lines.append("[REDACTED_SENSITIVE_LOG_LINE]\n")
            continue
        line = re.sub(r"(?i)\b(Bearer|Basic)\s+[^\s\"']+", "[REDACTED_AUTH]", line)
        line = re.sub(r"\b(?:xai-|sk-)[A-Za-z0-9_-]{16,}\b", "[REDACTED_KEY]", line)
        # Stderr URL queries are not business evidence. Do not retain signed URLs.
        line = re.sub(r"(https?://)[^/@\s]+:[^/@\s]+@", r"\1[REDACTED]@", line)
        line = re.sub(r"(https?://[^?\s\"<>]+)\?[^\s\"<>]*", r"\1?[REDACTED_QUERY]", line)
        lines.append(line)
    return {"text": "".join(lines), "truncated": truncated, "captured_bytes": len(prefix)}



def main():
    p = argparse.ArgumentParser()
    p.add_argument("--stderr", required=True)
    p.add_argument("--output", required=True)
    args = p.parse_args()
    # Read a bounded prefix plus one sentinel byte; never load an unbounded log.
    with open(args.stderr, "rb") as src:
        data = src.read(65537)
    result = safe_stderr(data, os.environ.get("GROK_API_KEY", ""))
    Path(args.output).write_text(json.dumps(result) + "\n")


if __name__ == "__main__":
    main()
