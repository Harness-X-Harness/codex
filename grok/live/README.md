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
  reasoning and a token-free same-thread continuation.
- `StructuredEdit`, `StructuredEditApprovalDeclined`,
  `StructuredEditPinnedPreviousModel` and `StructuredEditReplaceAll` observe
  exact authorized filesystem effects and effective approval denial.
- `AttachedImageHistory` submits an actual user image, checks a visual answer,
  then asks a different image-only fact in a text-only same-thread continuation.
- `ImageGenerationEdit` generates an image and edits it in the same Thread;
  fresh conversations independently judge each actual result image through
  the isolated image-capable fixture.
- `ShippedCatalog`, `ShippedStartup`, `ShippedPinned` and
  `ShippedChildCollaboration` consume unchanged embedded [shipped assets](../dist).
  Catalog-only makes no inference request. Startup preserves profile defaults;
  child delegation requests Ultra; pinned history retains the two-turn oracle.
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
  fresh result, opaque completed reasoning and recall in both whole eligible
  final responses using a required nullable `token` string matched to the actual tool result. A later final fragment does not erase earlier final content;
  commentary cannot substitute for final recall. The controlled Integration
  owner separately verifies unchanged encrypted replay.
- [Editing](sdk_edit_observer.go) checks successful terminal/final responses and
  actual file bytes after each turn and joined cleanup. Alternate replacement
  boundaries, multiple edits and auxiliary tools do not fail a correct result.
  The declined control still requires a matching denial and no committed effect;
  raw call pairing and file-change counts are not the Live oracle.
- [Images](image.go) require a completed public image and matching, fully decoded
  saved PNG. Fresh judge Threads receive actual pixels and an explicit proposition:
  a dog, then a dog wearing a red hat after the same-conversation edit. The SDK
  sends the formal `{"type":"boolean"}` output schema; only a completed JSON
  boolean `true` establishes the proposition. No producer history, word matching,
  breed dictionary or extra judge call is used. Raw extension calls, prepared history, artifact inventories and
  unequal hashes are not semantic proof or additional acceptance conditions.
  [Artifact checks](image_artifact.go) retain bounded decoding, safe saved-path
  access and public-payload/saved-byte agreement.
- [Child collaboration](shipped_child.go) requires inherited-marker recall,
  complete observed own-input coverage, a fresh child result, matching lineage
  and parent consumption through a required nullable `child_nonce` string. The
  parent result schema is not inherited by child turns. The parent's structured
  result is the terminal reply segment after the last finished public tool call
  (creation order), not concatenated intermediate sampling replies or the last
  parseable JSON. A successfully bound V2 spawn Started marker is also a tool
  boundary; unrelated or completed child activity is not. Its 4096-byte budget
  excludes intermediate text; full-turn identity/freshness checks and
  bounded diagnostics remain. Terminal null/invalid/multiple objects cannot
  fall back to an earlier positive answer. Missing/opaque coverage
  is unproven. Natural child prose, additional child UUIDs and intermediate tool
  activity are not failures by themselves;
  supplied or inherited results cannot count as fresh child work.
- [Web](search.go) requires completed public Web activity, successful terminals,
  structured articles (headline/URL) and a same-thread structured summary. These
  fields do not independently prove source accuracy, freshness or policy enforcement. Raw extension
  envelopes are not the business oracle. Native request/replay and controlled
  policy tests separately own protocol composition; prose alone is not hosted
  execution proof.

Task result schemas are supplied explicitly on each applicable turn. Attached-image
results contain nullable `color`/`shape` fields; reasoning uses nullable `token`;
the child parent uses nullable `child_nonce`; Web uses an article array and nullable
summary, followed by a summary object. These are task-neutral shapes, not schemas
containing expected fixture answers. One complete bounded JSON value is required;
duplicate/unknown/missing keys, wrong types, trailing values and text fallback are
rejected. Null records unknown information and cannot establish a required fact.
Business facts still come from actual images, tool results, child work and public
search activity. Basic, startup, catalog and filesystem edit checks retain their
direct oracles rather than gaining artificial structured answers. Diagnostics
retain the requested schema, public response and parse/semantic verdict separately.

Each semantic input is submitted once. Product-internal retries/recovery remain
product policy. Missing evidence does not authorize runner resubmission.

## Bounds, readback and cleanup

Source constants and tests own exact observation budgets. SDK frames cap at
16 MiB. Scenario-specific bounds remain independent: Basic completion callbacks
are bounded; reasoning bounds accepted identities and provisional replies;
editing bounds public observations and correlation; image observation bounds
terminal evidence and decoded bytes/pixels; child input coverage bounds events
and bytes; hosted search bounds public items and turn identities.
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

