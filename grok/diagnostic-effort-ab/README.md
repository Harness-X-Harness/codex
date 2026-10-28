# Temporary frozen initial-request replay — never merge

This observes initial HTTP acceptance only, not a completed Live task. It captures
a newly generated initial request from the same fixed binary, shipped profile,
grok-4.6 thread, high effort, original prompt, strict schema and native headers
that have previously succeeded. It does not claim to retain any old successful raw
request. Runtime artifact 12769806175, SHA256
47de14909531f35f7eacee71c1040bcb5d228ca1405ea2935327efef549e4c16;
harness e3c0812c7d47c567d42a181c921a30aa3c35cdc0.

Actual binary first talks only to a loopback capture, which returns a local 403.
The driver must stop with no tools executed and confirmed cleanup. This synthetic
403 is not an upstream observation. Raw request bytes and forwarded header values
remain solely in memory. The independent sender then sends exactly the same
initial bytes and headers at most twice, separated by at least 60 monotonic seconds.
It never gives upstream responses to the driver, never executes output tools, and
never submits continuation requests. No retries, redirects, alternate endpoint,
or extra probe. Unknown transport failures stop; only HTTP200/403 permit the
second planned observation. Requests consume budget before attempting connection.

Proof records unnormalized body SHA256, safe shape, header names but no values,
request IDs, status, bounded SSE event type names and measured timing. No raw
request/response, credential, cipher or private reasoning content is retained.
Two sends with identical body/header values do not imply stateless upstream:
request/session IDs and prompt cache key may affect deduplication or routing.
403 then200 disproves deterministic rejection of this fixed payload; other
outcomes do not identify caching, rate limiting, or a specific backend cause.

Before secrets are injected, existing actual-binary loopback roundtrip/schema/
header/zero-retry checks plus first-request-only capture must pass. Captured source
files and runtime bind this gate to execution. The current batch is at most two
real HTTP requests; cumulative count before it is 15. Formal oracles and permission
settings are unchanged. This branch is temporary and must never be merged.
