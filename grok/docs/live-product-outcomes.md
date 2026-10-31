# Live product outcomes and independent diagnostics

## Accepted revision, 2026-10-30

The user authorized implementation of a product-first review of all 18 Live
scenarios, not merely changing labels after three failures. The task should look
like useful work; backstage evidence must establish that the real capability
produced the result. Detailed diagnostics help explain failures but are not new
acceptance assertions. This document records the changed contract and its owners.
It is not a record of a successful run of the revised harness.

### Inventory and explicit replacements

| Existing scenario | Decision | Required outcome / retained owner |
| --- | --- | --- |
| `basic-primary` | Keep | One completed ordinary text task on the primary isolated route; `TestBasicFixture` |
| `basic-pinned` | Keep | Same capability on the pinned route, not inferred from primary |
| `reasoning-primary` | Simplify task | This shop's price lookup, two-kilogram total, then three-kilogram history continuation; `TestReasoningHistoryFixture/primary` |
| `reasoning-pinned` | Simplify task | Same real lookup/history proof on the pinned isolated route; `/pinned` |
| `structured-edit` | Combine | Update both opening-time occurrences in an ordinary notice, then confirm without mutation; `TestStructuredEditFixture/continuation` |
| `structured-edit-declined` | Keep, simplify wording | Actual matching denial and unchanged file, not just a refusal sentence; `/declined` |
| `structured-edit-pinned` | Keep, simplify wording | One actual file replacement on pinned; `/pinned` |
| `structured-edit-all` | Diagnostic-only duplicate | Main edit now owns both occurrences; offline `one_of_two` rejects partial edits; native `/replace_all` remains mandatory |
| `attached-image-pinned` | Simplify | Actual image input and correct left/right answers across two turns; public earlier observations are valid context, not forbidden leakage |
| `image-primary` | Simplify proof budget | Generate one dog, same-thread edit to two dogs; one fresh isolated judge sees both actual PNGs |
| `image-pinned` | Simplify proof budget | Same image capability on pinned; native `TestImagePinnedFixture` and paired-input mutation controls |
| `shipped-catalog` | Diagnostic-only duplicate | `ShippedStartup` already requires the identical complete catalog check; native `TestShippedCatalog` remains mandatory |
| `shipped-startup` | Keep | Unchanged shipped profile/catalog, bound default route and one ordinary completed turn |
| `shipped-pinned` | Simplify task | Same shop lookup/history task, with unchanged shipped catalog and pinned route; not replaced by the isolated case |
| `shipped-child` | Replace artificial task | Delegate an invoice check; actual child produces the correct invoice, result reaches parent, parent returns it; native positive plus parent-only and child-without-invoice negatives |
| `web` | Keep, simplify prompt | Real hosted Web completion and articles, then same-thread summary |
| `web-allowed` | Keep | Same outcome under allowed-domain policy; not inferred from plain Web |
| `web-excluded` | Keep | Same outcome under excluded-domain policy; not inferred from allowed policy |

The revised formal full selection is **16 scenarios / 28 top-level semantic
turns**, down from 18 / 32. The two diagnostic-only names remain explicitly
selectable. Smoke remains the same three scenarios. These are runner invocation
budgets, not limits on product-internal tool calls, transport retries or child
turns. No automatic semantic retry manufactures acceptance.

The [Story index](stories/README.md), [scenario catalog](../internal/scenarios/catalog.go)
and [mandatory native owner inventory](../internal/testresults/results.go) must
agree. Removing a formal duplicate does not remove its deterministic owner.

## Outcome, attribution and diagnostic completeness are separate

| Observation | Product proof | Attribution / diagnostic meaning |
| --- | --- | --- |
| Every required current-operation fact is supported | PROVEN | Scope is only this artifact, route, operation and run |
| A completed reply is wrong, an authorized edit is absent, or the turn explicitly fails | NOT_PROVEN; observed task failure | The failed boundary is known; defect ownership may still be inconclusive |
| Required delivery, completion or result evidence is absent | NOT_PROVEN | Insufficient evidence; not proof that no attempt occurred |
| Diagnostic queue drops, serialization/I/O fails, or bounded flush expires | Unchanged | Report incomplete diagnostics independently |
| Optional trace is absent while the mandatory semantic witness is sufficient | Unchanged | Do not create a new gate from telemetry presence |
| Earlier run passed, but the harness, binary or contract changed | Not evaluated for new candidate | Historical evidence cannot certify different bytes |

The process/run lifecycle is also independent: submitted, active, terminal,
callbacks joined and process reaped are different facts. A successful answer
cannot erase a contradictory terminal or unresolved owned cleanup. Conversely,
a later business-proof failure must not erase independently established partial
facts such as a child having completed its own turn.

The user-confirmed 2026-10-31 limit reduction separates task correctness from
observation resources. Complete answers are not rejected for a repeated
4096-byte allowance, Unicode width, article count or unrequested string
maxima. Exact invoice cents accept insignificant leading/trailing zeros; child
path syntax remains owned by the native protocol rather than a Live-only
length rule. Bound authoritative results, not unrelated history or consistent
duplicate representations, own semantic validation. Identity conflicts,
contradictory terminals, complete parsing, actual operations and cleanup remain
required. SDK frame/retention limits, child candidate/delivery budgets, image
memory/path safety and invocation/deadline limits remain resource boundaries.
Exhausting such a boundary leaves proof incomplete; no truncated prefix earns
acceptance. The formal and diagnostic scenario selections above are unchanged.

### Shop lookup and history

The prompt identifies this shop, the quantity and yuan; the registered capability
identifies the shop's price list. It does not prescribe a tool identifier, JSON
copying or an invented current market price. The formal amount schema is supplied
through the SDK. The price is generated only inside an accepted callback, so a
valid first answer with no accepted lookup means `history_lookup_not_observed`,
not demonstrated wrong arithmetic or memory. After a lookup, the first total and
continuation are compared with the actual price. Completed encrypted reasoning,
correlated tool result, unchanged native replay and the two correct totals remain
separate required facts. Model-facing instructions are simpler; the proof is not.

### Actual child contribution, not nonce choreography

The ordinary task is an importable invoice for three notebooks at 4.50 and two
pens at 1.25, total 16.00, with no tax/shipping. CSV is the requested business
artifact, not a hidden identifier protocol. There is no setup marker, UUID,
full-fork instruction or parent output schema to leak into the child.

A real, correctly bound child must complete its own correct invoice; that result
must enter the current parent's official input, followed by the parent's correct
durable answer. A parent-only correct calculation or a child saying “ready” does
not prove delegation. This is external contribution evidence, not a claim to know
the model's internal reasoning. Default full-fork, provider binding and per-turn
schema scope retain their own deterministic/native owners in the revised Story.

Stock creates/adopts a child before emitting public Started. Missing Started is
therefore not proof of no attempt. Discovery may use equivalent typed activity
or durable lineage hints. Raw callback identity/order helps establish delivery;
durable official messages, not raw provider text, own invoice semantics. Replayed
pre-delivery IDs cannot become fresh post-delivery evidence, and a history read
is not a callback-delivery fence. Missing evidence remains unproved rather than
being replaced with a parent answer or a diagnostic trace.

### Web history: useful context, not “every done must succeed”

Preserving truthful conversation context is essential; requiring every terminal
tool operation to have succeeded is not a product requirement. A known failed
operation could coherently remain a failed operation in history if the provider
accepts that representation. Unknown/corrupt/oversized records must never be
silently dropped or credited as successful searches.

The present Grok implementation uses `store=false` and projects history for
subsequent requests. Removing only the SSE rejection would leave the same
projection failure on the next request, and the public Web item does not carry a
failure status that the existing Live success oracle can safely reinterpret.
The reviewed [Web search documentation](https://docs.x.ai/developers/tools/web-search)
and [Responses documentation](https://docs.x.ai/developers/rest-api-reference/inference/responses)
did not establish a supported failed-Web next-input representation or a safe
omission rule. This is a contract-evidence gap, not a claim that the provider
cannot support recovery. No unsupported semantic relaxation is implemented.

The change instead preserves the existing acceptance predicate and adds finite
rejection reasons/status/action classes and size/cap metadata. The serializer is
still bounded and single-pass; raw queries, URLs, IDs and unknown status strings
are not copied into the error. The original rejected payload was not retained,
so these new diagnostics do not retrospectively identify its cause. If a supported
failure mapping is later established, it needs explicit next-request replay and
no-false-success tests before changing this boundary.

## Diagnostic contract and remaining observation gaps

The existing bounded bundle keeps run identity, source/binary/harness linkage,
submitted synthetic tasks, public outcomes, finite failure stages and capture
status. New emitted-function metadata can distinguish a visible model call from
no observed call even when dispatch or child creation fails before public Started.
It records correlation IDs, type/name, byte counts, JSON shape and known field
types, not arbitrary arguments or tool output. SDK-declared tools and effective
thread config explicitly say `provider_request_observed=false`: they are not an
actual outbound wire inventory. Controlled HTTP shape tests own that fact.

Capture is optional: an immutable sanitized snapshot is admitted to a bounded
queue, one worker performs I/O, and admission closes atomically with Close.
Admission references do not assert successful artifact persistence. The manifest
separately reports drops, I/O errors, outstanding work and flush timeout. A bounded
Close can return while the writer is blocked; `closed` does not mean joined or
complete. Detailed sink assertions belong in deterministic diagnostic tests,
never in the Live business oracle.

High-volume public deltas now retain counts/bytes and bounded metadata samples,
not repeated text fragments. Complete public items retain their existing
sanitized content. Fixed terminal, error, result, cleanup and invocation summaries
are independent of the ordinary event quota and appear in the final report and
manifest with hashed correlation identities. A completed terminal can therefore
remain visible alongside a failed business result and successful cleanup, even
when the trace is full. Deliberate sampling, unexpected capture loss and product
proof are separate; none of these diagnostic fields becomes a Live assertion.

Remaining limits are explicit: the pinned SDK discards child stderr; the public
stream cannot expose a rejected provider frame it never forwarded; opaque
backend behavior remains opaque; a crash may prevent final status or upload.
Do not label missing traces “no model attempt,” infer HTTP 403 from a controlled
shape probe, or invent raw payloads. Raw provider retention would require a
separately reviewed, bounded, secret-safe boundary; it is not added here.

## Focused lifecycle model and evidence boundary

This is an **unexecuted review sketch**, not a TLC result or refinement proof:

```tla
VARIABLES proof, lifecycle, diagnostics, queue, accepting, writerJoined

Capture == /\ accepting /\ Len(queue) < QueueBound
           /\ queue' = Append(queue, ImmutableSanitizedSnapshot)
           /\ UNCHANGED <<proof, lifecycle>>
Drop    == /\ diagnostics' = RecordFiniteDrop(diagnostics)
           /\ UNCHANGED <<proof, lifecycle, queue>>
Close   == /\ accepting' = FALSE /\ UNCHANGED <<proof, lifecycle>>
FlushTimeout == /\ diagnostics' = MarkIncomplete(diagnostics)
                /\ UNCHANGED <<proof, lifecycle, queue, writerJoined>>
```

Review obligations: diagnostic-only transitions stutter on product proof state;
acceptance never reads queue/status/files; admitted snapshots cannot change after
enqueue; admission and Close have one linearization boundary; each job is written
at most once in admitted order; all retained work and byte accounting are bounded;
timeout remains truthful after late completion. Writer progress assumes I/O
returns. Logical noninterference is not a physical zero-CPU/I/O-overhead theorem.

Controlled regressions cover immutability, Record/Close races, overflow, failed
sinks, blocked-final-manifest timeout and business noninterference. Child tests
separately force pre-delivery replay, phase-less tool rounds, reordered callbacks,
wrong lineage and correct-parent/absent-child alternatives. Each claimed pass
must name the actual tested revision; static review is not native/provider proof.

## Historical observations are immutable

At harness `9083841e`, source `d48ee14a`, runtime SHA256
`9fa7a1f263c9b9e054186761eb07379dfb03639872b27a6d053a6cd542b9bc1f`,
full run [41071724932](https://github.com/Harness-X-Harness/codex/actions/runs/41071724932)
was **15/18 pass, 3 fail, 0 skip**. The fourth failed Go test was parent `TestLive`,
not a fourth scenario. Shipped-pinned lacked an accepted lookup; shipped-child
lacked established child/result evidence; allowed-Web ended in a replay-validation
stream error after six valid completed Web items. The rejected raw item is absent.

Separately authorized unchanged one-scenario reruns
[41081849096](https://github.com/Harness-X-Harness/codex/actions/runs/41081849096),
[41081913355](https://github.com/Harness-X-Harness/codex/actions/runs/41081913355) and
[41081969875](https://github.com/Harness-X-Harness/codex/actions/runs/41081969875)
were **3/3 pass**. They do not turn the earlier full run into 18/18, establish a
root-cause fix, or prove this revised harness. No new paid Live accompanies this
implementation. Earlier local reconstruction/test artifacts were lost with an
executor reset; source rebuilt from exact base requires fresh review/tests.
