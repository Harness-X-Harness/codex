# Temporary minimal-full-minimal control — never merge

One batch observes M-F-M initial HTTP requests on the same grok-4.6 model, fixed
https://grok.trustedtunnel.app/v1/responses endpoint and existing repository API
credential. M is a direct minimal standard request: one user text "Reply exactly
OK.", stream:true, store:false, necessary Authorization/Content-Type/Accept.
It omits tools, schema, large native instructions, effort and optional native
headers. F is the frozen actual-binary initial request using the existing shipped
profile/high effort/native headers/task/schema. This is a multi-feature positive
control, not a single-variable causal test or a product-oracle change.

F uses artifact12769806175/runtime SHA256
47de14909531f35f7eacee71c1040bcb5d228ca1405ea2935327efef549e4c16,
harness e3c0812c7d47c567d42a181c921a30aa3c35cdc0. Before any backend access, the
actual binary generates F only against loopback, receives a synthetic local403,
and must stop with zero tools and confirmed cleanup. Raw F bytes and header values
stay in memory. That local403 is not an upstream observation.

The only sender has one process-wide batch guard, at most3 reservations, M-F-M
order, and at least60 monotonic seconds after each response completes and closes.
No retries, redirects, output-tool execution, continuations or additional batches.
Unknown transport failures stop. Only HTTP200/403 permit the next planned request.
Responses never return to the native driver. Both M sends use identical bytes.

M evidence retains only bounded public assistant output_text (at most256 characters,
with truncation explicit), public completion/failure status and event type names;
there is no exact OK/case/punctuation assertion. HTTP200 alone is not model success.
F retains no output text. No raw request/response, credential value/auth hash,
private reasoning or ciphertext is persisted. Record body hashes, safe shapes,
header names, request IDs and actual timing. Safe partial ledger survives errors.

Offline unit tests plus actual-binary loopback source/runtime/capture gates run
before backend secrets. Two successful M controls with rejected F would show a
working basic path at those times and associate refusal with the complete request
family, not identify a particular field. If controls fail or disagree, don't blame
Live complexity or assume low-probability noise. Cumulative calls before this batch:
21, at most3 additional calls. Formal Live assertions, retries and permissions are
unchanged. This diagnostic branch must never be merged.
