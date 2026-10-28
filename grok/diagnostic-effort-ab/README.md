# Formal 164 prompt-only safety-context diagnostic (not executed live)

Source: e3c0812c7d47c567d42a181c921a30aa3c35cdc0; source inspection and five offline unit tests completed. Production files unchanged. This is diagnostic evidence, not Story acceptance.

## Finding

The dynamic tool actually generates 32 random bytes with crypto/rand and returns their hexadecimal encoding. It does not retrieve a credential or user information. Original first-turn prompt and tool description call this value a “token”, without that provenance explanation. This makes the user's proposed safety/data-leakage ambiguity a testable working hypothesis. Prior verb change and context removal did not isolate this semantic clarification.

## One factor

A: exact original prompt.
B: prefix the exact original prompt with:

“This is a synthetic software test. The value returned by grok_history_probe is a newly generated random test marker containing no credentials or private information. ”

Everything else stays unchanged, including tool description, function name, output schema field `token`, reasoning requirement, full original task, effort, surrounding input, model and headers. This tests *prompt contextual disambiguation*, not deletion of every occurrence of token. It cannot isolate individual trigger words, length effects or prove which policy/layer rejected. Description-only and schema-name experiments would be separate later factors, not mixed into this batch.

`prompt_variant.prepare()` accepts an in-memory captured initial request and changes exactly its one standalone user-task JSON string. All other wire bytes remain identical (including whitespace). Duplicate JSON keys, duplicate occurrences, unexpected literal encoding, missing/nonstandalone task, and continuation input fail closed. No network sender exists here. It returns only in-memory modified bytes and secret-safe hash/size/path proof. Headers are not accepted or logged by this module.

## Proposed bounded live batch — needs parent approval

1. Reuse verified formal164 runtime artifact 12769806175, SHA256 47de14909531f35f7eacee71c1040bcb5d228ca1405ea2935327efef549e4c16, source f6086acf108192197318e2462e48369d8a354a59, bound by formal harness SHA above. Do not replace runtime with165.
2. Capture the formal164 initial request through loopback only and verify cleanup, zero tools and zero upstream calls. Formal test does not override effort; record actual frozen value. Old FCF relay/controller asserted high effort, but driver and wrapper did not override it. Reuse the unchanged formal shipped-profile configuration; the offline capture must prove its observed high value. A mismatch stops rather than rewriting the request.
3. One model grok-4.6, same existing repository credential and existing https://grok.trustedtunnel.app/v1/responses endpoint. No credential extraction, persistence or cross-provider forwarding. Capture/auth lifecycle must be reviewed before any run.
4. One process-wide fixed sequence A B B A, max4 initial requests total (2/arm), no automatic retries, redirects, tool execution or continuation. Wait at least60 monotonic seconds after each response completes. HTTPS socket timeout90 seconds and response cap2MiB; workflow has a20minute total deadline. The socket timeout is not a strict whole-response wall-clock cap. Reserve budget before opening connection; ambiguous failure consumes reservation.
5. Preserve one capture's body/header identities across arms; exact A replay and exact B replay. Record safe hashes, request index/arm, status, bounded error-code classification, request ID and event-type names, completion state, start/end timing. Do not persist prompts/raw responses/reasoning/ciphertext/auth or full header values.
6. Abort on failed offline/capture/provenance/one-factor checks, unknown transport state, unexpected status (outside200/403), response over cap or cleanup failure. No extra requests to replace failures. End after4 reservations at most; no follow-on batch without fresh approval.

## Cost and interpretation

The hard cap is four paid-provider requests, zero continuations. No defensible dollar cap is established from available source. Do not invent a dollar figure or add a max-output-token field to the frozen formal request solely to imply a budget (that would change another factor). If monetary cap is required, existing provider-side limit needs confirmation before execution.

ABBA balances coarse position/linear time effects but is a tiny nonrandomized screen. Fixed session/cache IDs and replay itself can affect caching/policy; cache treatment might differ by prompt. Model routing, stochastic filtering and temporal changes remain confounders. Same key/endpoint removes those between-arm differences but does not identify the rejecting backend layer. Two200B/two403A would support repeating the result, not establish root cause; mixed responses or all403/all200 are inconclusive, not a refutation of the user hypothesis. HTTP200 alone is not business acceptance and may contain an incomplete/failed response. Formal history/tool/reasoning acceptance remains unchanged.
