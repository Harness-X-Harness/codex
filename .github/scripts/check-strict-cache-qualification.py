"""One-shot fixture assertions, not a production CI helper."""
import json
from pathlib import Path
import sys

root = Path(sys.argv[1])


def stats(case, phase):
    document = json.loads((root / f"{case}-{phase}.json").read_text())
    assert document["cache_location"].startswith("Local disk"), "unexpected backend"
    raw = document["stats"]
    values = {key: raw[key] for key in (
        "compile_requests", "compile_fails", "cache_writes", "cache_read_errors",
        "cache_write_errors", "cache_timeouts")}
    values["rust_hits"] = raw["cache_hits"]["counts"].get("Rust", 0)
    values["rust_misses"] = raw["cache_misses"]["counts"].get("Rust", 0)
    values["cache_errors"] = sum(raw["cache_errors"]["counts"].values())
    assert not any(values[key] for key in (
        "cache_read_errors", "cache_write_errors", "cache_timeouts", "cache_errors")), values
    return values


previous = None
cold_messages = None
for case, success, code in (
    ("cold", True, "clippy::len_zero"),
    ("warm", True, "clippy::len_zero"),
    ("source-denied", False, "clippy::panic"),
    ("config-denied", False, "clippy::disallowed_methods"),
    ("warn-cap", True, "clippy::len_zero"),
    ("strict-cap", False, "clippy::len_zero"),
):
    before, after = stats(case, "before"), stats(case, "after")
    if previous is not None:
        assert all(before[key] >= previous[key] for key in before), f"{case}: counters reset"
    assert all(after[key] >= before[key] for key in before), f"{case}: counters regressed"
    delta = {key: after[key] - before[key] for key in before}
    assert delta["compile_requests"] > 0, f"{case}: no observed wrapper request"
    status = int((root / f"{case}-exit.txt").read_text())
    assert (status == 0) == success, f"{case}: unexpected exit {status}"
    messages = [row["message"] for line in (root / f"{case}-compiler.jsonl").read_text().splitlines()
                if (row := json.loads(line)).get("reason") == "compiler-message"]
    level = "warning" if success else "error"
    assert any((message.get("code") or {}).get("code") == code and message["level"] == level
               for message in messages), f"{case}: expected diagnostic absent"
    if not success:
        assert delta["compile_fails"] > 0, f"{case}: no observed compiler failure"
    if case == "cold":
        assert all(before[key] == 0 for key in ("rust_hits", "rust_misses", "cache_writes"))
        assert delta["rust_misses"] > 0
        cold_messages = messages
    if case == "warm":
        assert delta["rust_hits"] > 0, "warm: actual Rust cache hit absent"
        assert messages == cold_messages, "warm: diagnostics differ"
    print(json.dumps({"case": case, "exit": status, "lint": code, "delta": delta}, sort_keys=True))
    previous = after
print("Qualified this fixture: actual warm hit with equal diagnostics; changed inputs rejected.")
print("Negative failures do not identify the cache lookup outcome or establish universal equivalence.")
