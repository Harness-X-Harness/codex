# Temporary native-header A/B control — never merge

Both arms use the original shipped profile/catalog, grok-4.6 thread, original
first-turn history-probe prompt, strict token schema and dynamic probe. Neither
overrides effort (shipped defaults to high). Sandbox and approval are unchanged.
A keeps the previous relay header policy. B additionally forwards exactly three
observed native headers: x-codex-beta-features, x-codex-turn-metadata and
x-codex-window-id. Values go only to the same fixed endpoint and are never logged.
This tests the header group, not the causality of any individual header.

Fixed binary artifact 12769806175, runtime SHA-256
47de14909531f35f7eacee71c1040bcb5d228ca1405ea2935327efef549e4c16,
harness e3c0812c7d47c567d42a181c921a30aa3c35cdc0. Config default 4.7 remains
unchanged, while actual thread and outgoing request must be 4.6.

This follow-up executes only B then A; at most four real HTTP sends (two per arm), no further pair, retries or
redirects. Errors never refund budget. A second request must be the paired tool
result. Unknown failure or incomplete cleanup stops the batch. Before backend
secret injection, actual-binary loopback verifies both profiles, same task/schema,
expected effort, tool rounds, cleanup, 403 zero-retry and the header policy.
Normalized body equality allows only previously proven fresh-session cache key,
temporary cwd, random probe token and typed msg/fco identity mapping across both
requests. Call IDs, arbitrary IDs, prompt text and cipher remain untouched.
Unknown body differences prevent real execution.

All checks run before aggregation; safe reports survive failure. Evidence retains
header names only from a fixed vocabulary, unknown-name count, bounded request
shape, digest, status, UTC and request IDs. No credentials, private reasoning,
ciphertext or complete message text is saved. Host and Content-Length are rebuilt
by the HTTP library, not omitted upstream. SSE buffering and 90-second socket
timeout remain shared transport limitations.

Previous batches: effort A/B 2 sends, prompt A/B 2, concurrent isolated/shipped
control 2; all initial HTTP403. The last batch proved both profiles lost precisely
the three headers above. The first header pair used 3 sends: A passed its tool round, B received initial403. This follow-up only reverses order; cumulative real requests before it: 9. The
workflow is temporary/dispatch-only; never merge or modify the formal Live oracle.
