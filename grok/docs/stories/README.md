# Provider Stories and executable owners

These are retained user-visible contracts, not a ledger of passing runs. A
fake App Server test validates its observer; actual Codex with controlled HTTP
validates composition; Rust tests own product semantics; direct Facts observe
the backend; identified Codex + real Grok Live establishes only its executed
Story at its recorded subject. Historical success does not prove the current
binary, platform or harness. See [commands and evidence interpretation](../../TESTING.md).

The controlled Integration owners below run in `./tests/integration`
with `-tags=integration`. `TestLive/<name>` runs in `./tests/live` with
`-tags=live` and explicit credentials/opt-in. Scenario names and eligibility
come from the [Go catalog](../../internal/scenarios/catalog.go).

| Retained Story | Deterministic / controlled owner | Actual backend owner and scope |
| --- | --- | --- |
| [Configured Grok fixture completes a text turn](grok-provider-fixture-turn.md) | [`TestBasic*`](../../live/basic_test.go); [`TestBasicFixture`](../../tests/integration/basic_test.go) uses both fixture models | `TestLive/basic-primary`, `basic-pinned`; direct Basic Facts are separate |
| [Encrypted reasoning survives same-thread continuation](grok-encrypted-reasoning-history-continuation.md) | [`TestReasoningHistoryPublicProtocol`](../../live/reasoning_history_test.go), SDK observer controls; [`TestReasoningHistoryFixture`](../../tests/integration/reasoning_test.go) checks three controlled HTTP requests and encrypted replay | `TestLive/reasoning-primary`, `reasoning-pinned`; direct encrypted-replay Facts do not prove product recall |
| [Workspace file edit](grok-structured-edit.md) | [`TestStructuredEdit*`](../../live/structured_edit_test.go), [`TestSDKEditObserver*`](../../live/sdk_edit_observer_test.go); [`TestStructuredEditFixture`](../../tests/integration/structured_edit_test.go); [Rust structured-edit suites](../../../codex-rs/core/tests/suite/structured_edit.rs) own actual routing/approval/filesystem semantics | `TestLive/structured-edit`, `structured-edit-declined`, `structured-edit-pinned`, `structured-edit-all` retain actual bytes/hash, terminal and effective-denial oracles |
| [User-attached image conversation](grok-user-attached-image-history.md) | [`TestAttachedImageHistory` and pixel/gate controls](../../live/attached_image_script_test.go); [`TestAttachedImagePinnedFixture`](../../tests/integration/attached_image_test.go) | `TestLive/attached-image-pinned`; actual user image input and distinct same-thread visual follow-up |
| [Image generation and same-thread history edit](grok-image-generation-history-edit.md) | [`TestGrokImageGenerationEdit` and false-evidence controls](../../live/image_test.go); [`TestImagePinnedFixture`](../../tests/integration/image_test.go); [Rust image preparation](../../../codex-rs/core/src/image_preparation.rs) and [public image integration](../../../codex-rs/app-server/tests/suite/v2/imagegen_extension.rs) | `TestLive/image-primary`, `image-pinned` use isolated image profiles/catalogs, with fresh-conversation recognition of generated and edited pixels; unchanged shipped-profile composition remains unproven |
| [Shipped profile startup](grok-provider-profile-startup.md) | [`TestShippedStartupAndChildRejectIncompleteEvidence`, `TestShippedPinnedUsesRetainedHistoryScenario`](../../live/shipped_test.go); [`TestShippedThreadSource`](../../tests/integration/shipped_test.go); [Rust provider binding](../../../codex-rs/app-server/tests/suite/v2/grok_provider_binding.rs) | `TestLive/shipped-startup`, `shipped-pinned` use unchanged shipped assets; full tool composition needs its own executed evidence |
| [Exact process catalog](provider-catalog-app-visibility.md) | [`TestShippedCatalogRequiresCompleteDTOs`](../../live/shipped_test.go); [`TestShippedCatalog`](../../tests/integration/basic_test.go); [Rust Grok Model DTO suite](../../../codex-rs/app-server/tests/suite/v2/grok_model_list.rs) | `TestLive/shipped-catalog` starts the selected binary and checks catalog only, with zero paid inference |
| [Provider-bound fork](grok-provider-bound-thread-fork.md) | [Rust `grok_provider_binding`](../../../codex-rs/app-server/tests/suite/v2/grok_provider_binding.rs): `grok_fork_keeps_provider_binding_and_isolates_branch_failure`, `grok_cold_fork_preserves_pinned_model_under_different_default`, override/conflict and stock controls | No new Go/full-backend fork claim. Existing Rust owner is required; backend acceptance remains unproven without an identified run |
| [Cold resume](grok-provider-bound-cold-restart-resume.md) | Same Rust suite: `grok_cold_restart_resume_keeps_provider_binding`, including missing-profile failure and restored-profile recovery | No duplicate Go internal-correctness suite and no inferred backend success |
| [Local compaction](grok-provider-bound-compaction-continuation.md) | Same Rust suite: `grok_manual_compaction_keeps_provider_binding`; [core Grok compaction](../../../codex-rs/core/tests/suite/grok_compaction_tests.rs) owns dialect/replay | No duplicate Go compaction suite and no inferred backend success |
| [Child collaboration](grok-provider-binding-lifecycle.md) | [`TestShippedStartupAndChildRejectIncompleteEvidence`](../../live/shipped_test.go); [`TestShippedChildFixture`](../../tests/integration/child_test.go); [Rust public child listener](../../../codex-rs/app-server/tests/suite/v2/grok_child_public_listener.rs) and provider inheritance owners | `TestLive/shipped-child`: one setup plus one natural delegation; child lineage, fresh result and parent consumption remain material |
| [Hosted Web/X search](#hosted-webx-search) | [`TestHostedSearch*`](../../live/search_test.go); [`TestHostedSearchFixture`](../../tests/integration/hosted_search_test.go); [Rust request/replay](../../../codex-rs/core/tests/suite/grok_hosted_search_tests.rs), [public Web activity](../../../codex-rs/app-server/tests/suite/v2/grok_search_activity.rs), [retired-X ingress and absence of public fork events](../../../codex-rs/app-server/tests/suite/v2/grok_search_activity.rs) | `TestLive/web`, `web-allowed`, `web-excluded` retain positive acceptance; product `x`, `x-window` are retired under #414. Direct SearchPolicy/SearchReplay Facts remain separate |

## Hosted Web/X search

As a Grok user, I can request supported hosted **Web** search, receive a complete
final answer, and continue on the same Provider-bound thread. Positive acceptance
requires one first turn and one follow-up, each submitted once, matching identities,
completed public Web activity, successful terminals, nonempty settled final
answers and a completed same-thread continuation. Raw extension envelopes and
byte-identical internal replay are not additional Live acceptance conditions.
Final text is collected from eligible final-answer items,
ignoring commentary and accepting multiple final fragments. Target history is
matched by unique submitted Turn identity, not array position or total length.
Allowed/excluded domains remain distinct controlled policies, each with a
controlled composition case.
Prose alone, unrelated/cross-turn activity or missing cleanup cannot establish
that Story. Native and controlled HTTP tests separately own request/replay
compatibility and rejection of synthetic local dispatch/output. Actual backend
relevance and policy enforcement
require separate observations; a mock or a 2xx Facts response is insufficient.

The [request policy owner](../../../codex-rs/codex-api/src/grok_search.rs)
rejects unsupported restrictions before transport. Nonempty allow/exclude lists
are mutually exclusive and limited to five domains; empty lists are omitted.
Stock cached/indexed/live hints map to one Grok wire mode, without claiming
equivalent backend enforcement. [Hosted replay](../../../codex-rs/protocol/src/grok_hosted.rs)
keeps canonical Web identity/payload without local output synthesis and rejects
oversized items at the stock 10,000 estimated-token (40,000 UTF-8 byte) cap.
This is a bytes/4 estimate, not a measured Grok tokenizer guarantee; truncation
or reclassification cannot repair oversized replay.

**RETIRE under #414:** Hosted X has no truthful official typed representation at
the pinned SDK baseline. The product stops advertising `x_search`, rejects explicit
X configuration and unexpected X ingress, removes `xSearch` public items and the
fork-only `item/searchActivity` method, and rejects old X history explicitly rather
than silently dropping it or pretending it is Web/prose/local-tool execution.
[`TestHostedSearchFixture`](../../tests/integration/hosted_search_test.go) retains Web positives plus retired-X negative controls.
The [Rust public stream tests](../../../codex-rs/app-server/tests/suite/v2/grok_search_activity.rs)
own emitted-ABI absence. [Rollout forward/reverse readers](../../../codex-rs/rollout/src/recorder_tests.rs),
[cached/paginated history](../../../codex-rs/thread-store/src/local/thread_history/read_tests.rs)
and [request-build history](../../../codex-rs/core/src/client_retired_history_tests.rs)
own explicit legacy-X rejection without synthetic local pairing. Generated
schema/consumer consistency is independently required.

The earlier public search-specific activity promise is explicitly narrowed: Web
uses official item started/completed notifications; early `item/searchActivity`
timing is no longer supported. Historical #362/#364 results are context, not a
claim that their full previous public activity contract survives this retirement.

The offline unknown-public-type control in [search tests](../../live/search_test.go)
requires generic fail-closed behavior through the ordinary Web runner; it earns
no positive Hosted X or formal Live credit. The SDK ignores unsubscribed unknown
methods, so Rust emitted-stream/schema tests establish that `item/searchActivity`
is absent. No SDK pin/schema relaxation, stdout filtering or shadow protocol
consumer is used. Cleanup does not prove delivery of every raw callback.

Direct backend X Facts (`x`, `x-window`, `x-replay`) remain useful independently of
the retired product. They do not certify a product source, binary or supported Live Story.

## Formal and diagnostic scope

Full and smoke Live selections are defined only by the [Go catalog](../../internal/scenarios/catalog.go).
Omitted cases have not passed. Reasoning history uses the model default without
a turn effort override; it still requires actual reasoning and encrypted history.
Catalog-only tests need no inference; authenticated Live invocation
still follows the common opt-in contract. Linux and macOS packages have separate
native checks; a Linux backend run does not certify macOS execution.

[#339](https://github.com/Harness-X-Harness/codex/issues/339) retains exact
package-to-Live provenance. No documentation change, deterministic pass or
workflow declaration constitutes executed Live acceptance.
