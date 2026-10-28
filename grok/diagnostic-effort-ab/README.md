# Temporary 60-second session interval control — never merge

Both arms use the original shipped profile/catalog, grok-4.6 thread, original
first-turn history-probe prompt, strict token schema and dynamic probe. Neither
overrides effort (shipped defaults to high). Sandbox and approval are unchanged.
Both sessions forward the same three observed native headers:
x-codex-beta-features, x-codex-turn-metadata and x-codex-window-id.
Values go only to the same fixed endpoint and are never logged.
After the first session completes and cleanup is confirmed, the controller waits
at least 60 seconds measured with a monotonic clock before starting session two.
The safe summary records the measured interval. This probes interval association,
not a proven rate limit or a causal explanation of HTTP403.

Fixed binary artifact 12769806175, runtime SHA-256
47de14909531f35f7eacee71c1040bcb5d228ca1405ea2935327efef549e4c16,
harness e3c0812c7d47c567d42a181c921a30aa3c35cdc0. Config default 4.7 remains
unchanged, while actual thread and outgoing request must be 4.6.

This follow-up executes only A then B with identical settings; at most four real HTTP sends (two per arm), no further pair, retries or
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

Previous batches used 12 real sends in total. In the two latest header pairs,
whichever policy ran first succeeded, while the immediately following session
received initial403. This group changes to two native-header sessions separated
by 60 seconds; it is exploratory, not proof of rate limiting. The workflow is
temporary/dispatch-only; never merge or modify the formal Live oracle.
