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
  exact authorized filesystem effects and paired history.
- `ImageGenerationEdit` observes generation, canonical history selection and
  a distinct same-thread edit through an isolated image-only fixture.
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
  fresh result, opaque completed reasoning and eligible final recall in both
  turns. Raw reasoning text is not encrypted evidence. The controlled
  Integration owner independently verifies unchanged encrypted replay.
- [Editing](sdk_edit_observer.go) correlates approval, function/result and file
  change, checks exact bytes/hash after each turn and after joined cleanup, and
  rejects an unauthorized effect even if a later terminal succeeds.
- [Images](image.go) reconcile canonical raw call/output, completed public image
  and settled history. [Artifact checks](image_artifact.go) require fully decoded
  PNG, exact public payload/saved-byte equality, fresh nonsymlink regular files,
  readability and distinct edited output. Prepared history may be resized or
  re-encoded, so it has its own digest; exact call identity and selection bind
  that rendition to the public artifact and later edit. A pre-lifecycle validation
  error cannot prove an image, and a started image still needs its terminal.
- [Child collaboration](shipped_child.go) requires inherited-marker recall,
  complete observed own-input coverage, a fresh child result, matching lineage
  and parent consumption. Missing/opaque coverage is unproven. Parent-supplied
  or inherited results, tool-assisted marker lookup and exact credited-terminal
  conflicts cannot earn credit; unrelated later orchestration is not failure.
- [Web](search.go) requires canonical hosted calls, successful terminals,
  nonempty settled answers and unchanged same-thread replay. Local redispatch,
  synthetic output or prose alone cannot prove hosted execution.

Each semantic input is submitted once. Product-internal retries/recovery remain
product policy. Missing evidence does not authorize runner resubmission.

## Bounds, readback and cleanup

Source constants and tests own exact observation budgets. SDK frames cap at
16 MiB. Scenario-specific bounds remain independent: Basic completion callbacks
are bounded; reasoning bounds accepted identities and provisional replies;
editing bounds arguments, outputs and correlation; image observation bounds
frames, calls, artifact inventory and decoded bytes/pixels; child input coverage
bounds events and bytes; hosted search bounds calls and replay identities.
A budget overflow leaves the result unproven, not evidence of a product/service
limit. Image-only fixtures do not prove a general packaged view-image-before-edit
route. Controlled fixtures do not establish real-backend relevance or policy
enforcement.

Cleanup uses SDK `CloseContext` with a separate deadline. `CleanupPending` means
a join is unproved; fixtures/snapshots stay available instead of being removed
under a running process or admitted callback. Constructor failure may provide no
join handle. Follow TESTING's retained-snapshot instructions before manual cleanup.

Returned evidence contains safe subject metadata, timestamps, fixed stages,
counts, booleans and approved fixture/artifact hashes. Private text, credentials,
raw traffic, opaque IDs and image/reasoning payloads remain in memory and are not
returned. Partial terminal/effect facts must stay truthful after a later failure;
failure attribution is inconclusive unless independently established. See the
[current failure-evidence contract](../TESTING.md#live-failure-evidence).

Offline fake-process tests prove the observer. Actual-binary controlled HTTP
Integration proves its exercised product composition. Rust tests own product
semantics. Only an identified actual-backend invocation can establish the
corresponding real-backend result. No source entrypoint, old run or green
unrelated test supplies that evidence.
