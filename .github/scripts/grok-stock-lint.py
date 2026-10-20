#!/usr/bin/env python3
"""Two lint-only exceptions for exact stock rust-v0.163.0; never alter build inputs.

Clippy 1.95 produced these exact edits in PR #430 run 39364280793,
artifact 12239550587, including its whitespace and `as _` coercion.
The let-and-return diagnostic also reproduces on stock in run 39368206131
(workspace lock versions normalized only). The unused import is recorded from
PR CI, not independently reached by that stock control. No lint is disabled.
"""

import hashlib
import pathlib
import subprocess
import sys

# Original blobs belong to stock 83086188ae0614bd882818ff1aa23f3194962012.
EXCEPTIONS = (
    ("codex-rs/app-server/src/message_processor.rs",
     "c9858cd9d513c69a917f6505410ee89b16bdd955",
     b'                    let time_provider =\n                        app_server_time_provider(outgoing.clone(), thread_state_manager.clone());\n                    time_provider\n', b'                    \n                    (app_server_time_provider(outgoing.clone(), thread_state_manager.clone())) as _\n', 'a5c951af8bef6b65496c0d9c3917105570173afc'),
    ("codex-rs/core/tests/suite/scenarios.rs",
     "cfad8149ad70f43482453d59e7128ac975bbca03",
     b"use codex_protocol::openai_models::ReasoningEffort;\n", b"", '842ae7aeae97f6912e364f972ff8044061f176aa'),
)


def blob(data):
    return hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()


def run(root, command):
    if not command:
        raise ValueError("expected the existing lint command")
    files = []
    for name, original_sha, old, new, changed_sha in EXCEPTIONS:
        path = root / name
        original = path.read_bytes()
        if blob(original) != original_sha or original.count(old) != 1:
            raise RuntimeError(f"stock lint exception requires reassessment: {name}")
        changed = original.replace(old, new, 1)
        if blob(changed) != changed_sha:
            raise RuntimeError(f"invalid recorded lint delta: {name}")
        files.append((path, original, changed))
    try:
        for path, _, changed in files:
            path.write_bytes(changed)
        result = subprocess.run(command).returncode
    finally:
        unexpected = []
        for path, original, changed in files:
            try:
                actual = path.read_bytes()
            except OSError:
                unexpected.append(str(path))
                continue
            if actual == changed:
                path.write_bytes(original)
            elif actual != original:
                unexpected.append(str(path))
        if unexpected:
            raise RuntimeError("unexpected lint edits retained: " + ", ".join(unexpected))
    # Other files are never restored or exempted; the existing clean gate checks them.
    return result


if __name__ == "__main__":
    root = pathlib.Path(__file__).resolve().parents[2]
    sys.exit(run(root, sys.argv[1:]))
