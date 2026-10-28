# Formal 164 original-versus-everyday scenario diagnostic

Source: e3c0812c7d47c567d42a181c921a30aa3c35cdc0. Production files unchanged. This is first-request HTTP diagnostic evidence, not Story acceptance or a completed tool task.

## Actual A/B content

A prompt: “Call grok_history_probe, reason about its returned token, then return JSON with the exact token as the token field, or null if unavailable.”

A tool: grok_history_probe; description “Return a fresh token for this history probe.”; no arguments. A output schema: object with required token:string|null and no additional properties.

B prompt: “Look up the price per kilogram of apples. How much would two kilograms cost?”

B repurposes the same one fixture tool as apple_price_lookup; description “Return the current price of apples in yuan per kilogram.”; same empty-object parameters. B output schema: object with required amount:number|null and no additional properties. This schema is supplied out of band, not requested in the user text. Native flat function names and the existing routing-description wrapper are updated consistently using formal164's projection, not an arbitrary new tool mechanism.

## Scope of comparison

This is a whole-task scenario comparison, NOT a one-word causal experiment. Only four JSON value ranges may change: one standalone user task, the uniquely bound fixture tool's name and description, and text.format.schema. Model, effort, other tools and input context, headers, identities and all bytes outside these ranges stay unchanged. The transformer rejects ambiguous/missing fixtures, mismatched schema or parameters, unknown native wrappers, name collisions, duplicated old task/name context and continuation requests. It does not recursively replace token strings or weaken native transport checks.

Only the initial request is sent: no tool result or price has been provided at this point. A200 cannot establish that the model correctly calculated a price, used a tool, remembered history or completed the task. We do not execute tools or send continuations. The old disclaimer and identifier-only proposals are withdrawn.

`prompt_variant.prepare()` performs the fixed four-range transform offline, retains request bytes only in memory, and returns allowlisted path/hash/size proof. No credential or transport API is present in that module. The existing controller and native loopback gate are reused; no new test framework or production oracle changes.

## Proposed bounded live batch — needs parent approval

1. Reuse verified formal164 runtime artifact 12769806175, SHA256 47de14909531f35f7eacee71c1040bcb5d228ca1405ea2935327efef549e4c16, source f6086acf108192197318e2462e48369d8a354a59, bound by formal harness SHA above. Do not replace runtime with165.
2. Capture the formal164 initial request through loopback only and verify cleanup, zero tools and zero upstream calls. Formal test does not override effort; record actual frozen value. Old FCF relay/controller asserted high effort, but driver and wrapper did not override it. Reuse the unchanged formal shipped-profile configuration; the offline capture must prove its observed high value. A mismatch stops rather than rewriting the request.
3. One model grok-4.6, same existing repository credential and existing https://grok.trustedtunnel.app/v1/responses endpoint. No credential extraction, persistence or cross-provider forwarding. Capture/auth lifecycle must be reviewed before any run.
4. One process-wide fixed sequence A B B A, max4 initial requests total (2/arm), no automatic retries, redirects, tool execution or continuation. Wait at least60 monotonic seconds after each response completes. HTTPS socket timeout90 seconds and response cap2MiB; workflow has a20minute total deadline. The socket timeout is not a strict whole-response wall-clock cap. Reserve budget before opening connection; ambiguous failure consumes reservation.
5. Preserve one capture's body/header identities across arms; exact A replay and exact B replay. Record safe hashes, request index/arm, status, bounded error-code classification, request ID and event-type names, completion state, start/end timing. Do not persist prompts/raw responses/reasoning/ciphertext/auth or full header values.
6. Abort on failed offline/capture/provenance/allowed-scenario-diff checks, unknown transport state, unexpected status (outside200/403), response over cap or cleanup failure. No extra requests to replace failures. End after4 reservations at most; no follow-on batch without fresh approval.

## Cost and interpretation

The hard cap is four paid-provider requests, zero continuations. No defensible dollar cap is established from available source. Do not invent a dollar figure or add a max-output-token field to the frozen formal request solely to imply a budget (that would change another factor). If monetary cap is required, existing provider-side limit needs confirmation before execution.

ABBA balances coarse position/linear time effects but is a tiny nonrandomized screen. Fixed session/cache IDs and replay itself can affect caching/policy; cache treatment might differ by prompt. Model routing, stochastic filtering and temporal changes remain confounders. Same key/endpoint removes those between-arm differences but does not identify the rejecting backend layer. Two200B/two403A would support repeating the result, not establish root cause; mixed responses or all403/all200 are inconclusive, not a refutation of the user hypothesis. HTTP200 alone is not business acceptance and may contain an incomplete/failed response. Formal history/tool/reasoning acceptance remains unchanged.
