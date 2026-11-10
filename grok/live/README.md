# Explicit-binary Live observers

This package owns bounded observations of user-visible results through the
selected Codex binary's App Server v2 API. Start with the [Story index](../docs/stories/README.md)
for consumer semantics and code/test ownership; use [TESTING](../TESTING.md) for
commands, execution tiers, evidence interpretation and package/snapshot rules.
The [scenario catalog](../internal/scenarios/catalog.go) alone owns selectable
names, eligibility and invocation budgets. The SDK version is pinned in
[go.mod](../go.mod).

## Interface and authority

Callers supply a `Subject` with an absolute binary path, expected executable
SHA-256 and declared source/harness/target/environment metadata. `Options` adds
the selected fixture model, Provider endpoint and environment-provided key.
An explicit deadline is required. Direct calls must keep the supplied binary
stable; canonical Integration and Live/CLI entrypoints instead create an owned,
byte-verified [execution snapshot](../TESTING.md#integration-actual-codex-and-controlled-grok-http).
A digest proves bytes, not independently verified release provenance.

- `Basic` uses an isolated text-only profile/catalog from
  [providerfixture](../internal/providerfixture), independent of shipped assets.
- `ReasoningHistory` adds fresh authorized tool output, completed encrypted
  reasoning and a same-thread apple-price follow-up without a new lookup.
- `StructuredEdit`, `StructuredEditApprovalDeclined`,
  `StructuredEditPinnedPreviousModel` and `StructuredEditReplaceAll` observe
  exact authorized filesystem effects and effective approval denial.
- `AttachedImageHistory` submits an actual user image, checks a visual answer,
  then asks a different visual fact in a text-only same-thread continuation.
- `ImageGenerationEdit` generates an image and edits it in the same Thread;
  one fresh producer-isolated judge sees both actual PNGs in order through
  the isolated image-capable fixture.
- `ShippedCatalog`, `ShippedStartup`, `ShippedPinned` and
  `ShippedChildCollaboration` consume unchanged embedded [shipped assets](../dist).
  Catalog-only makes no inference request. Startup preserves profile defaults;
  child delegation uses an ordinary invoice task; pinned history retains the two-turn oracle.
- `HostedSearch` observes supported Web with default, allowed-domain or
  excluded-domain policy. [Hosted X is retired](../docs/stories/README.md#hosted-webx-search);
  provider-only X Facts cannot complete a product Story.

The stock SDK owns process launch, framing, typed protocol, callbacks, stream
attachment and cleanup. No failure switches clients or rewrites stdout. Product
Rust owners retain routing, model policy, authorization, sandbox, filesystem
mutation, media preparation, persistence and lifecycle semantics. Go observers
do not reimplement them.

## Observer-owned correctness

SDK success alone is insufficient. Results need independent thread/turn/call
correlation, successful terminal evidence and the Story's actual result.
Consistent duplicates may represent one event; conflicts, stale evidence,
cross-turn data and unrelated terminals cannot manufacture success. Settled
thread reads reconcile history but cannot synthesize a missing observed terminal.
Valid unauthorized requests are denied without effect. Malformed requests may
be wire-rejected; the observer still needs every positive result criterion.

- [Reasoning](reasoning_history.go) requires a completed authorized named tool,
  per-run apple price, opaque completed reasoning and price-based totals in both
  whole eligible final responses using a required nullable numeric `amount`
  matched to two and three kilograms respectively. A later final fragment does not erase earlier final content;
  commentary cannot substitute for final recall. The controlled Integration
  owner separately verifies unchanged encrypted replay.
- [Editing](sdk_edit_observer.go) checks successful terminal/final responses and
  actual file bytes after each turn and joined cleanup. Alternate replacement
  boundaries, multiple edits and auxiliary tools do not fail a correct result.
  The declined control still requires a matching denial and no committed effect;
  raw call pairing and file-change counts are not the Live oracle.
- [Images](image.go) require completed public images and matching fully decoded
  saved PNGs. Generate one dog and edit to two dogs on the same thread; one
  fresh isolated judge receives both actual images in order. The SDK supplies
  two required boolean fields; both must be true. There is no producer history,
  word matching or extra judge turn, and this is not an independent double judge.
  [Artifact checks](image_artifact.go) retain bounded decoding, safe saved-path
  access and public-payload/saved-byte agreement; unequal hashes are not semantic proof.
- [Child collaboration](shipped_child.go) requires a real correctly bound child
  to complete its own correct invoice, deliver it into the current parent's
  official input, and the parent to return a subsequent correct durable invoice.
  Parent-only answers and a child saying “ready” cannot pass. No nonce, UUID,
  full-fork instruction or inherited parent schema is part of the ordinary task.
  Raw identity/order establishes a delivery witness; official durable messages
  own the invoice semantics. Started is one discovery hint, not the unique
  oracle. Missing evidence is unproven, not proof of no delegation attempt.
- [Web](search.go) requires completed public Web activity, successful terminals,
  structured articles (headline/URL) and a same-thread structured summary. These
  fields do not independently prove source accuracy, freshness or policy enforcement. Raw extension
  envelopes are not the business oracle. Native request/replay and controlled
  policy tests separately own protocol composition; prose alone is not hosted
  execution proof.

Task result schemas are supplied explicitly on each applicable turn. Attached-image
results contain nullable `color`/`shape` fields; reasoning uses nullable numeric `amount`;
Web uses an article array and nullable
summary, followed by a summary object. These are task-neutral shapes, not schemas
containing expected fixture answers. One complete JSON value is required;
duplicate/unknown/missing keys, wrong types, trailing values and text fallback are
rejected. Null records unknown information and cannot establish a required fact.
Child uses the requested CSV invoice rather than an imposed JSON protocol.
Business facts still come from actual images, tool results, child work and public
search activity. Basic, startup, catalog and filesystem edit checks retain their
direct oracles rather than gaining artificial structured answers. Diagnostics
retain the requested schema, public response and parse/semantic verdict separately.

Answer length, Unicode byte width, article count and decimal spelling are not
correctness conditions. The SDK and observation owners bound incoming data and
retained evidence; validators consume the complete selected result, never a
truncated prefix. An observation-budget failure leaves validation incomplete,
not an incorrect-answer finding. Authoritative final fragments are selected by
bound identity and phase after checking conflicting identities; discarded
commentary, unrelated history and consistent duplicate representations do not
consume a second answer or evidence allowance.

Each semantic input is submitted once. Product-internal retries/recovery remain
product policy. Missing evidence does not authorize runner resubmission.

## Bounds, readback and cleanup

Source constants and tests own exact observation budgets. SDK frames cap at
16 MiB. Scenario-specific bounds remain independent: Basic completion callbacks
are bounded; reasoning bounds accepted identities and provisional replies;
editing bounds retained unique relevant evidence and early unbound observations;
image observation bounds terminal evidence and decoded bytes/pixels; child
delivery/identity evidence has explicit event and byte bounds; hosted search
retains bounded transport observations and correlated item identities without
an independent cap on successful Web operations.
A budget overflow leaves the result unproven, not evidence of a product/service
limit. Isolated image fixtures do not prove unchanged shipped-profile composition
or require a particular optional view-image route. Controlled fixtures do not establish real-backend relevance or policy
enforcement.

Cleanup uses SDK `CloseContext` with a separate deadline. `CleanupPending` means
a join is unproved; fixtures/snapshots stay available instead of being removed
under a running process or admitted callback. Constructor failure may provide no
join handle. Follow TESTING's retained-snapshot instructions before manual cleanup.

Returned verdict evidence contains safe subject metadata, timestamps, stages,
counts, booleans and fixture/artifact hashes. Separate bounded task diagnostics
retain submitted synthetic inputs, public responses and tool results, phase/error
observations, and fixture image/file attachments before validation and cleanup.
They explain the outcome; their contents and capture failures are not new business
assertions. `Options.Diagnostics` enables this sink; the CLI accepts
`--diagnostics-dir`, and the tagged runner uses `GROK_LIVE_DIAGNOSTICS_DIR` with one
subdirectory per selected catalog scenario. Both Live workflows retain the bundle.

The bundle contains incremental `events.jsonl`, an atomically refreshed
`manifest.json` (closed/completeness, limits and capture errors), and fixed-name
PNG/text attachments. It excludes credentials/auth/environment dumps, unrelated
history, private reasoning/ciphertext and full raw wire envelopes. Public reasoning
configuration/capability metadata is not private reasoning. Task IDs associate
only the selected operation's public observations. The existing verdict summary
remains suitable for logs; the attached diagnostic bundle intentionally includes
synthetic task content and public backend responses and shares Actions artifact
access/30-day retention. Do not use it for arbitrary private user uploads.

Chat/output deltas contribute total counts and byte counts, plus at most 16
metadata-only samples per method; complete public items own sanitized text.
Sampling is explicit, not unexpected loss or a source for reconstructing replies.
Compact last-terminal/error, fixture-result, SDK-cleanup and invocation summaries
survive ordinary event quotas in the manifest and final report. Correlation there
uses hashes, and a successful terminal is not conflated with a validated result
or completed cleanup. These summaries are diagnostic, never acceptance inputs.

Capture limits are explicit rather than silent: 4096 events, 64 KiB per event,
32 attachments of at most 32 MiB each, 128 MiB total. Oversized events are marked,
not treated as complete evidence. Capture failure is reported independently in the
command report and cannot replace the original business failure. Partial
terminal/effect facts remain truthful after later failure; attribution stays
inconclusive unless independently established. Pinned SDK subprocess stderr is
not exposed by its public API and is explicitly marked unavailable; sanitized,
bounded Go-runner stderr is retained separately. Hard runner loss can prevent
upload; an unfinished manifest must not be called complete. See the
[current failure-evidence contract](../TESTING.md#live-failure-evidence).

Offline fake-process tests prove the observer. Actual-binary controlled HTTP
Integration proves its exercised product composition. Rust tests own product
semantics. Only an identified actual-backend invocation can establish the
corresponding real-backend result. No source entrypoint, old run or green
unrelated test supplies that evidence.

The [product-outcome decision](../docs/live-product-outcomes.md) separates required
user outcomes, NOT_PROVEN attribution and optional diagnostic completeness, and
records the explicit formal-suite consolidations. Emitted-function diagnostics
retain bounded shape/correlation metadata, not arbitrary arguments or outputs.
SDK tool declarations and effective thread config are labelled as not observing
the actual provider request. The existing SDK stderr limitation remains.
