# Provider Stories and executable owners

These are retained user-visible contracts, not a ledger of passing runs. A
fake App Server test validates its observer; actual Codex with controlled HTTP
validates composition; Rust tests own product semantics; direct Facts observe
the backend; identified Codex + real Grok Live establishes only its executed
Story at its recorded subject. Historical success does not prove the current
binary, platform or harness. See [commands](../../TESTING.md) and the
[assertion inventory](../test-layer-inventory.md).

`TestIntegration/...` and `TestIntegrationInternal/...` below run in `./live`
with `-tags=integration`. `TestLive/<name>` runs in `./tests/live` with
`-tags=live` and explicit credentials/opt-in. Scenario names and eligibility
come from the [Go catalog](../../internal/scenarios/catalog.go).

| Retained Story | Deterministic / controlled owner | Actual backend owner and scope |
| --- | --- | --- |
| [Configured Grok fixture completes a text turn](grok-provider-fixture-turn.md) | [`TestBasic*`](../../live/basic_test.go); `TestIntegration/BasicFixture` uses both fixture models | `TestLive/basic-primary`, `basic-pinned`; direct Basic Facts are separate |
| [Encrypted reasoning survives same-thread continuation](grok-encrypted-reasoning-history-continuation.md) | [`TestReasoningHistoryPublicProtocol`](../../live/reasoning_history_test.go), SDK observer controls; `TestIntegration/ReasoningHistoryFixture` checks three controlled HTTP requests and encrypted replay | `TestLive/reasoning-primary`, `reasoning-pinned`; `reasoning-no-effort-pinned` is diagnostic-only; direct encrypted-replay Facts do not prove product recall |
| [Structured exact-match file edit](grok-structured-edit.md) | [`TestStructuredEdit*`](../../live/structured_edit_test.go), [`TestSDKEditObserver*`](../../live/sdk_edit_observer_test.go); `TestIntegration/StructuredEditFixture`; [Rust structured-edit suites](../../../codex-rs/core/tests/suite/structured_edit.rs) own actual routing/approval/filesystem semantics | `TestLive/structured-edit`, `structured-edit-declined`, `structured-edit-pinned`, `structured-edit-all` retain exact bytes/hash, authorization and history oracles |
| [Image generation and same-thread history edit](grok-image-generation-history-edit.md) | [`TestGrokImageGenerationEdit` and false-evidence controls](../../live/image_test.go); `TestIntegration/ImagePinnedFixture`; [Rust image preparation](../../../codex-rs/core/src/image_preparation.rs) and [public image integration](../../../codex-rs/app-server/tests/suite/v2/imagegen_extension.rs) | `TestLive/image-primary`, `image-pinned`; the isolated fixture does not prove every packaged general-profile view-image path |
| [Shipped profile startup](grok-provider-profile-startup.md) | [`TestShippedStartupAndChildRejectIncompleteEvidence`, `TestShippedPinnedUsesRetainedHistoryScenario`](../../live/shipped_test.go); `TestIntegrationInternal/ShippedThreadSource`; [Rust provider binding](../../../codex-rs/app-server/tests/suite/v2/grok_provider_binding.rs) | `TestLive/shipped-startup`, `shipped-pinned` use unchanged shipped assets; full tool composition needs its own executed evidence |
| [Exact process catalog](provider-catalog-app-visibility.md) | [`TestShippedCatalogRequiresCompleteDTOs`](../../live/shipped_test.go); `TestIntegration/ShippedCatalog`; [Rust Grok Model DTO suite](../../../codex-rs/app-server/tests/suite/v2/grok_model_list.rs) | `TestLive/shipped-catalog` starts the selected binary and checks catalog only, with zero paid inference |
| [Provider-bound fork](grok-provider-bound-thread-fork.md) | [Rust `grok_provider_binding`](../../../codex-rs/app-server/tests/suite/v2/grok_provider_binding.rs): `grok_fork_keeps_provider_binding_and_isolates_branch_failure`, `grok_cold_fork_preserves_pinned_model_under_different_default`, override/conflict and stock controls | No new Go/full-backend fork claim. Existing Rust owner is required; backend acceptance remains unproven without an identified run |
| [Cold resume](grok-provider-bound-cold-restart-resume.md) | Same Rust suite: `grok_cold_restart_resume_keeps_provider_binding`, including missing-profile failure and restored-profile recovery | No duplicate Go internal-correctness suite and no inferred backend success |
| [Local compaction](grok-provider-bound-compaction-continuation.md) | Same Rust suite: `grok_manual_compaction_keeps_provider_binding`; [core Grok compaction](../../../codex-rs/core/tests/suite/grok_compaction_tests.rs) owns dialect/replay | No duplicate Go compaction suite and no inferred backend success |
| [Child collaboration](grok-provider-binding-lifecycle.md) | [`TestShippedStartupAndChildRejectIncompleteEvidence`](../../live/shipped_test.go); `TestIntegrationInternal/ShippedChildFixture`; [Rust public child listener](../../../codex-rs/app-server/tests/suite/v2/grok_child_public_listener.rs) and provider inheritance owners | `TestLive/shipped-child`: one setup plus one natural delegation; child lineage, fresh result and parent consumption remain material |
| [Hosted Web/X search](#hosted-webx-search) | [`TestHostedSearch*`](../../live/search_test.go); `TestIntegration/HostedSearchFixture`; [Rust request/replay](../../../codex-rs/core/tests/suite/grok_hosted_search_tests.rs), [public Web activity](../../../codex-rs/app-server/tests/suite/v2/grok_search_activity.rs), [public X activity](../../../codex-rs/app-server/tests/suite/v2/grok_x_search_activity.rs) | `TestLive/web`, `web-allowed`, `web-excluded`, `x`, `x-window`; direct SearchPolicy/SearchReplay Facts remain separate |

## Hosted Web/X search

As a Grok user, I can request hosted Web or X search under the supported policy,
receive a complete final answer, and continue on the same Provider-bound thread.
An accepted execution requires:

- One configured first turn and one same-thread follow-up, each submitted once.
- Matching Provider/model/thread/turn identities, successful terminal evidence
  and authoritative nonempty final output for both turns.
- A causally matched supported canonical hosted call, with consistent duplicates
  credited once. Public search-activity notifications alone cannot prove that
  canonical call; local custom dispatch/output cannot earn hosted credit.
- Correct permitted request projection: Web allow `reuters.com`, Web exclude
  `example.com`, or X window `2026-08-01` through `2026-08-15` are distinct fixed
  controls. Rust and controlled HTTP own exact product request/replay checks.
- Bounded secret-safe evidence and cleanup. Missing, stale, conflicting,
  cross-turn or post-settlement new evidence must not become successful proof.

Public activity timing, transient UI lifecycle and mixed Web/X ownership belong
to the existing Rust owners linked above. The Go observer independently checks
canonical raw business evidence and same-thread continuation. Neither this
observer nor a 2xx Facts response proves result relevance, actual allow/exclude
policy enforcement, date-bound inclusivity or cold-restart persistence.

The original stdout stream, including public `xSearch`, must reach the pinned
SDK unchanged. Its unsupported variant is a required Integration failure until
[#408](https://github.com/Harness-X-Harness/codex/issues/408) is resolved under
separate SDK review; filtered output and fake-process success cannot close that
gap. [#388](https://github.com/Harness-X-Harness/codex/issues/388) owns the retained
SDK search Story. Historical #228/#234 and [#331](https://github.com/Harness-X-Harness/codex/issues/331)
runs supply context only; none certifies the rebuilt current subject.

## Formal and diagnostic scope

Full Live retains 19 catalog scenarios; smoke selects four for targeted feedback.
Omitted cases have not passed. `reasoning-no-effort-pinned` remains exploratory
under [#384](https://github.com/Harness-X-Harness/codex/issues/384), outside formal
full proof. Catalog-only tests need no inference; authenticated Live invocation
still follows the common opt-in contract. Linux and macOS packages have separate
native checks; a Linux backend run does not certify macOS execution.

[#339](https://github.com/Harness-X-Harness/codex/issues/339) retains exact
package-to-Live provenance. No documentation change, deterministic pass or
workflow declaration constitutes executed Live acceptance.
