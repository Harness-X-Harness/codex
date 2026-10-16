# Grok test ownership and redundancy audit

This is a source/selector inventory for #401, not a mutable success ledger.
Commands are in [TESTING](../TESTING.md), user contracts in the
[Story index](stories/README.md). A test name containing “native” is not enough
to classify it: pure request/evidence helpers remain offline; tests that execute
Codex are explicit Integration owners.

## Boundary and change decisions

| Boundary | Owner and before/after decision | Evidence it cannot supply |
| --- | --- | --- |
| SDK process, framing, typed protocol and callbacks | Pinned `llm-go/codexsdk`; retain a minimal Grok consumer launch/cleanup/failure/isolation contract and all safety/evidence assertions | SDK success alone is not a completed Grok Story |
| Product routing, model policy, fork/resume/compaction, sandbox and public activity | Existing scoped Rust suites plus #414 ABI/history retirement controls; no Go reimplementation | Controlled native success is not current real-provider behavior |
| Fake App Server observer and local HTTP Facts contracts | Default `go test -race ./...`; all material scripted positive/negative cases retained | Real Codex composition, paid backend or verified package acceptance |
| Actual Codex + controlled HTTP | Nine actual tagged owners in `tests/integration`; shared mechanics in `internal/integrationfixture`, observer units in `live` | Actual backend behavior or current release certification |
| Direct backend Facts | Two actual `facts`-tagged roots in `tests/facts`; independent CLI and workflow remain | Product behavior, artifact provenance or Live completion |
| Actual Codex + actual Grok | `TestLive/<catalog-name>`, tagged `live`; existing business functions and evidence DTOs reused | Unselected Stories, other platforms, other source/package subjects |

The previous runtime job repeated offline `./live` tests already run in its Go
job. The runtime now selects only Integration; the reusable Go job runs the
whole default module once per formal proof subject. The shared JSON checker
also preserves the previous explicit presence requirements for
`live:TestReasoningHistoryPublicProtocol` and
`live:TestSDKEditObserverPositiveStory`, using full package-qualified names.
Other passing tests cannot hide either missing owner; neither is executed twice. Package acceptance retains
its five actual package consumer checks, because packaged launcher/assets/native
runner constitute a different boundary. Development pushes and PR merge refs
are different subjects; formal PR merge refs are excluded from the fast PR
workflow to avoid running the same default suite twice on that merge.

## Explicit KEEP / REPLACE / RETIRE decisions

- **KEEP:** Basic, reasoning/encrypted replay, structured edits/authorization/exact
  bytes, image ancestry, shipped catalog/profile/child and positive hosted Web.
  No identity, terminal, authorized effect, byte/hash, history, cleanup or
  provenance oracle is removed merely because the SDK is typed.
- **REPLACE:** selector-only `live/TestIntegration*` wrappers with nine actual
  owners in `tests/integration`; two credentialed Facts roots with actual
  `tests/facts` implementations. Shared setup and pure fixture assertions move
  to `internal/integrationfixture`; no production test-only facade is added.
- **RETIRE (explicit #414 product decision):** Hosted X advertisement, its
  `xSearch` public DTO and fork-only `item/searchActivity` method. No truthful
  official hosted-X representation exists at the pinned baseline. Formal Live
  drops `x`/`x-window`; supported Web uses official item lifecycle notifications.
  The earlier early-public-search activity promise is narrowed, not silently
  claimed preserved. Unexpected X and old persisted X fail explicitly without
  fake local dispatch, relabeling or dropping history.
- **KEEP as diagnostics only:** exact original-wire legacy `xSearch` SDK decode
  rejection, cleanup, timeout/other-error/business-precedence negative controls.
  They earn no positive hosted or formal Smoke/Full credit. Direct backend X
  Facts remain independent and unchanged.

The stdout filter remains removed. The SDK pin/strict schema is unchanged.
Its unknown-method ignore policy means Rust emitted-output/schema tests own
absence of `item/searchActivity`; the Go decoder alone cannot prove that fact.

## Acceptance mapping for #410–413

This maps obligations to source owners, not to assumed passing runs. The
[Story index](stories/README.md) remains the semantic index; the
[Go catalog](../internal/scenarios/catalog.go) remains the only executable
selection registry.

| Issue | Implemented source ownership | Execution/evidence still required |
| --- | --- | --- |
| [#410](https://github.com/Harness-X-Harness/codex/issues/410) | Catalog metadata/dispatch plus [selection controls](../internal/scenarios/catalog_test.go); [discovery CLI](../cmd/grok-scenarios/main.go); Go-derived matrices in [formal Live](../../.github/workflows/grok.yml), [manual Live](../../.github/workflows/grok-live.yml) and [Facts](../../.github/workflows/grok-facts.yml). Smoke/full/diagnostic eligibility and stable subtest selectors are documented in TESTING. | A source-valid matrix is not an executed workflow. Required event checking must accompany each actual selection; omitted scenarios receive no credit. |
| [#411](https://github.com/Harness-X-Harness/codex/issues/411) | Before/after owner map, retained-oracle tables and bug-class cross-check below; offline/runtime duplication removed without replacing Rust product owners. The explicit #414 X retirement and test-only #408 diagnostic boundary are recorded above. | Fresh default and actual-binary Integration race results at the candidate subject; timings and counts must identify their source. Historical local measurements below do not establish current race/native pass or paid-cost improvement. |
| [#412](https://github.com/Harness-X-Harness/codex/issues/412) | Every retained Story links its automated owner and boundary in the Story index, including supported hosted Web and explicit product-X retirement. Fork/resume/compaction retain existing Rust ownership. Catalog-only observes no inference. | Identified real-backend runs for positive Stories; legacy X rejection never satisfies positive hosted acceptance. |
| [#413](https://github.com/Harness-X-Harness/codex/issues/413) | [Tagged Live runner](../tests/live/live_test.go), shared [command execution](../internal/livecommand/run.go), [preflight controls](../cmd/grok-live/tagged_test.go), [JSON event checker](../internal/testresults) and independent [Facts](../facts/README.md). Trusted workflows verify artifacts before the same Go assertions. | Trusted smoke and separately invocable full execution at the declared subject, safe failure artifacts and measured API costs/durations. Neither docs nor mocks establish these outcomes; no paid run is claimed by this audit. |

The pinned SDK baseline is `github.com/ronhuafeng/llm-go`
`v0.2.1-0.20261014071218-8ade447507bc` in [go.mod](../go.mod).
[Public-stdio rejection controls](../live/search_test.go)
(`TestHostedSearchPublicXSearchExpectedRejection` and
`TestHostedSearchExpectedRejectionRejectsOtherFailures`) exercise the actual
pinned decoder against synthetic public notifications. The
[classification controls](../live/sdk_xsearch_test.go) reject missing causes,
other variants, RPC impersonation, decorated errors, observed or SDK-retained
business contradictions, artifact failures, timeouts, pending callbacks and
unproved process cleanup. Fatal decoding can discard undelivered raw callbacks;
zero pending callbacks is cleanup evidence, not complete raw delivery/audit. These controls are
secret-free prerequisites, not real Codex or backend evidence.

Formal [run 38845628133](https://github.com/Harness-X-Harness/codex/actions/runs/38845628133)
at `4a7823ce8e5b1ffbf0a98eb919b0932c1753051f` failed the previous X positive
expectation. Its generic protocol-end report does not prove an actual SDK
shutdown cause, and the revised acceptance does not retroactively turn that run
green. Fresh candidate evidence must be collected separately.

## Binary-dependent owners: exact before/after map

Descendants retain material business assertions and controlled request limits,
except the explicitly approved product-X retirement. No rejection earns positive
Story credit. Real executable-backed tests live only in `tests/integration`;
pure fixture/observer controls remain credential-free default tests.

| Previous owner | Required Integration owner | Positive/negative scope retained |
| --- | --- | --- |
| `TestNativeBasicFixture` | `TestBasicFixture` | Both fixture models; provider request shape, nonempty final, correct identity and successful terminal |
| `TestNativeStructuredEditFixture` | `TestStructuredEditFixture` | Continuation, pinned, declined and replace-all; real file bytes/hash and paired tool/output history |
| `TestNativeReasoningHistoryFixture` | `TestReasoningHistoryFixture` | Both models; named tool, fresh paired result, three HTTP requests, unchanged encrypted history and token-free continuation |
| `TestNativeHostedSearchFixture` | `TestHostedSearchFixture` | Web and Web-excluded retain policy, canonical hosted evidence, two turns and no local redispatch. Retired X has preflight/unsolicited-ingress negatives, no advertisement/local redispatch/fake credit; Rust owns public ABI/history closure |
| `TestNativeImagePinnedFixture` | `TestImagePinnedFixture` | Pinned generation and edit; real codec/artifact hashes, exact history lineage and four controlled HTTP requests |
| `TestNativeShippedCatalog` | `TestShippedCatalog` | Complete shipped DTO catalog, process identity; no inference |
| `TestNativeShippedThreadSource` | `TestShippedThreadSource` | Shipped source/profile/model binding and controlled startup/history; public startup plus independent typed durable-source observation now use two explicit local invocations (previously one private fixture), never a semantic retry |
| `TestNativeShippedChildFixture` | `TestShippedChildFixture` | Fresh child result succeeds; parent-supplied result rejected; source/lineage/nonce authority |
| `TestNativeFirstRequestShape` | `TestFirstRequestShape` | Primary/pinned isolated/shipped history and edit; pinned no-effort diagnostic; denied terminal and bounded existing sampling attempts |

Source: actual [Integration package](../tests/integration), with reusable
[fixture mechanics](../internal/integrationfixture). `--integration` in
[`grok-test-results`](../cmd/grok-test-results/main.go) requires all nine exact
package-qualified owner events. Package smoke uses an exact five-owner subset: Basic,
StructuredEdit, ShippedCatalog, ShippedChild and FirstRequestShape. A selected
owner cannot be satisfied by a sibling, parent, skip, incomplete run or absent
subtest. A missing binary is failure, never a green skip.

## Retained offline tests and subcase oracles

The named parent selectors below retain their table-driven descendant cases.
The linked source is the subcase authority; no duplicate manually maintained
case-name registry is introduced. Outcome counts below include parent and
nested test events, not just leaf scenarios.

| Source / named selectors | Assertion ownership and marginal value |
| --- | --- |
| [`basic_test.go`](../live/basic_test.go): `TestBasicCompletesMatchingTurn`, `TestBasicDeniesUnauthorizedAction`, `TestBasicProcessIsolation`, `TestBasicRejectsInsufficientEvidence` | Minimal SDK consumer launch/cleanup/isolated env; successful bound turn; unauthorized requests denied; wrong identity, missing output/terminal, malformed or excessive traffic cannot earn completion. `TestMain` retains owned fake-process mode. |
| [`reasoning_history_test.go`](../live/reasoning_history_test.go): `TestReasoningHistoryPublicProtocol` | Both models' scripted modes retain fresh authorized token/tool pairing, encrypted completion, N/N+1 identity, single token-free continuation, replay reconciliation, bounded authority and secret-safe failure. SDK lifecycle success cannot replace these predicates. |
| [`sdk_reasoning_observer_test.go`](../live/sdk_reasoning_observer_test.go): `TestSDKHistoryObserverProvisionalReconciliation`, `CompletedToolAuthority`, `FailurePrefixRecovery`, `CallCapacity` (same `TestSDKHistoryObserver` prefix) | Early/settled correlation, completed tool ownership, truthful partial progress, accepted-call bounds and consistent duplicate handling. These are application evidence rules, not SDK queue implementation tests. |
| [`structured_edit_test.go`](../live/structured_edit_test.go): `TestStructuredEditScenarios`, `TestStructuredEditRejectsFalseEvidence` | Four entry points and scripted positive/negative modes; exact effects, approval, bytes/hash, same-thread history, stale/conflicting evidence and no resubmission. |
| [`sdk_edit_observer_test.go`](../live/sdk_edit_observer_test.go): `TestSDKEditObserverPositiveStory`, `EarlyDeclineReconciliation`, `DuplicateAndConflict`, `RetainedFirstTurn`, `BudgetsAndSafeEvidence`, `IdentityAndContinuation`, `ConcurrentSnapshots`, `IncompleteProof` (same prefix) | Causal call/output/change proof, sticky denial, consistent replay, material identity, retained first-turn facts, cleanup-stable bytes and concurrent snapshots. Exact incidental transport stage strings are not a Story oracle. |
| [`structured_edit_wire_test.go`](../live/structured_edit_wire_test.go): `TestStructuredEditArgumentsRejectMalformedReplaceAll` | Application-owned edit argument/schema admission; malformed replace-all cannot reach valid effects. |
| [`integrationfixture/structured_edit_test.go`](../internal/integrationfixture/structured_edit_test.go): `TestNativeEditResultHistoryRejectsInvalidOutput`, `TestNativeEditResultHistoryPreservesReplay`, `TestNativeStructuredEditDeclaration`, `TestNativeEditFailureDetail` | Pure fixture-oracle tests stay default despite historical “Native” names: declaration, paired output history, consistent replay, safe unsuccessful-terminal detail. |
| [`image_test.go`](../live/image_test.go): `TestGrokImageGenerationEdit`, `TestImageGenerationEditRejectsFalseEvidence`; shared [image scripts](../live/image_script_test.go) | Primary/pinned generation/edit; raw/public/history image pairing, genuine fully decodable PNGs, preparation-vs-artifact digest distinction, selected previous-image lineage, filesystem safety, corruption and secret-safe bounded rejection. |
| [`search_test.go`](../live/search_test.go): `TestHostedSearchRetainsEachScenarioAndSingleContinuation`, `RejectsMissingConflictAndLocalExecution`, `RejectsContradictorySettledEvidence`, `AcceptsConsistentDelayedPriorTerminal`, `PublicXSearchExpectedRejection`, `ExpectedRejectionRejectsOtherFailures`, `RejectsCustomExecutionAtEveryObservationBoundary`, `RetainsTurnScopedCreditedIdentities`, `IdentityBudgetsRemainPerTurn` (same prefix) | Every supported Web policy; causal canonical calls and finals across two turns; public-X-only cannot earn canonical credit; wrong, local, stale, post-fence or conflicting evidence fails. SDK framing ownership does not replace hosted-origin authority. |
| [`shipped_test.go`](../live/shipped_test.go): `TestShippedCatalogRequiresCompleteDTOs`, `TestShippedStartupAndChildRejectIncompleteEvidence`, `TestShippedPinnedUsesRetainedHistoryScenario` | Entire DTO/profile assets; startup identity/terminal; child inherited context, freshness/lineage and parent consumption; pinned encrypted-history reuse. |
| [`integrationfixture/child_test.go`](../internal/integrationfixture/child_test.go): `TestShippedChildDeclarationSchemaBoundary` | Pure declaration schema control remains offline; independently rejects malformed tool declarations before composition. |
| [`integrationfixture/request_shape_test.go`](../internal/integrationfixture/request_shape_test.go): `TestFirstRequestShapeSummary`, `TestFirstRequestEffortPresence`, `TestFirstRequestHistoryDeclaration` | Pure bounded request summary and exact presence/declaration semantics remain offline; no network/CLI prerequisite. |
| [`failure_test.go`](../live/failure_test.go): `TestFailureDiagnosticsExcludePrivateContent`, `TestFailureDiagnosticsPreserveUnknownErrors` | Safe known categories/status only, unknown errors remain unknown; never emit prompt/model text/IDs/private error details. |
| [`facts/probe_test.go`](../facts/probe_test.go): `TestProbeTextUsesExplicitFixtureAndObservesCompletion`, `TestProbeReplayPreservesOpaqueValueAndTypedContent`, `TestProbeTextRequiresSemanticCompletion`, `TestProbeStopsReadingOversizedResponseBeforeEOF`, `TestProbeReplayFailuresDoNotResubmitOrLeak`, `TestProbeRejectsRedirectsAndCancelledTransport` | Local HTTP routing, semantic completion, opaque replay, read bounds, cancellation, redirect refusal and no retry/leak. Independent consumer boundary, retained. |
| [`facts/search_test.go`](../facts/search_test.go): `TestSearchPolicyPreservesEachRetainedWireFixture`, `TestSearchReplayPreservesSupportedCallsWithoutLocalOutput`, `TestSearchReplayMissingOrInvalidEvidenceStopsWithoutResubmission`, `TestSearchReplayFailurePreservesSafePriorEvidence`, `TestSearchUnknownFixtureMakesNoRequest`, `TestSearchReplayWebActionRepresentations`, `TestSearchReplayRejectsContinuationLocalExecution`, `TestSearchReplayRejectsNonHostedCustomCallsWithoutResubmission`, `TestSearchReplayAllowsOtherSupportedHostedKind` | Five policy projections, Web and four supported X call names, valid optional Web action fields, replay admission/identity, no local outputs, malformed/incomplete failure and truthful partial facts. |
| [`facts/shipped_test.go`](../facts/shipped_test.go): `TestShippedRoutesKeepRequestSlugsAndStopOnFailure` | Exact shipped request slugs; aliases diagnostic; stop on first failed route. |
| [`cmd/grok-live/run_test.go`](../cmd/grok-live/run_test.go): `TestCommandExecutesEveryOwnedScenario`, `TestCommandRejectsPreflightWithoutStartingRuntime`, `TestCommandRetainsFailedScenarioWithoutResubmission`, `TestCommandDeadlineStopsAnIncompleteScenario`, `TestCommandInterruptRetainsCancelledResult`, `TestCommandHelpAndEvidenceWriteFailure`, `TestCommandReportsBoundedFailureDiagnostics` | Compile/invoke actual compatibility CLI with fake App Server; all selections, subject/budget/preflight, cleanup on timeout/interrupt, output failure and redaction. Retained external JSON contract, not duplicate product Live. |
| [`cmd/grok-facts/run_test.go`](../cmd/grok-facts/run_test.go): `TestCommandInvokesEachExistingScenario`, `TestCommandPreflightMakesNoBackendRequest`, `TestCommandReportsFailureWithoutContinuationOrRawError`, `TestCommandDeadlineAndRedirectRemainNonSuccess`, `TestCommandCancellationAndUnwritableEvidence` | Actual independent CLI with local HTTP; request budgets, preflight isolation, cancellation/redirect/output failure and safe report schema. |

New deterministic owners cover [catalog validation/dispatch](../internal/scenarios),
[explicit executable selection](../internal/testkit),
[JSON event checking](../internal/testresults), [checker CLI](../cmd/grok-test-results),
and [tagged Live preflight](../cmd/grok-live/tagged_test.go). They reject duplicate
catalog entries, unknown/ineligible selection, implicit skips and repeat/empty
execution without making paid calls.

`TestBackendFacts` and `TestBackendSearchFacts` retain their direct-backend
subcases in `tests/facts` under the `facts` tag, both selecting and invoking the shared catalog.
Their previous independent invocation tables are removed; the same four
non-search and seven search observations remain. Exact selector changes are:

- `TestBackendFacts/basic_primary` → `basic-primary`; `basic_pinned` →
  `basic-pinned`; `encrypted_typed_replay` → `encrypted-replay`;
  `shipped_routes` → `shipped-routes`.
- `TestBackendSearchFacts/web_allowed` → `web-allowed`; `web_excluded` →
  `web-excluded`; `x_window` → `x-window`; `web_replay` → `web-replay`;
  `x_replay` → `x-replay`. The `web` and `x` names are unchanged.

These entries no longer emit skipped-test events in the default suite;
explicitly selecting them without setup fails. No backend probe or invocation
budget is added or retired, and none enters required offline coverage.

## Bug-class cross-check

| Material bug class | Retained consumer oracle / independent product owner |
| --- | --- |
| Untrusted action or unauthorized file change | Basic denial, edit sticky approval/bytes/hash, search no local dispatch; Rust safety/sandbox/edit composition |
| Wrong Provider/model/thread/turn or child lineage | Basic, reasoning, shipped and child negative fixtures; Rust provider binding and child public listener |
| Missing/failed terminal or final output | Every positive scenario observer; retained truthful partial facts; JSON lifecycle checks. Legacy X rejection requires actual SDK cause/cleanup only in offline diagnostics; it cannot count as formal completion |
| Stale replay, conflict or cross-turn credit | Reasoning provisional/history controls, edit identity/history, hosted per-turn fence and call identity |
| Hosted call mistaken for local execution | Search raw/public distinction and no-local-call/output controls; Rust supported Web replay and retired-X ingress/ABI/history negatives |
| Corrupt/misbound media or edited bytes | Image full decode/file/hash/lineage and structured-edit byte oracle; Rust image preparation and filesystem owners |
| Resource leak or unbounded retention | SDK consumer process isolation/cleanup, per-observer bounds, capped binary/result input, cancellation/deadline tests |
| Secret disclosure or fabricated provenance | Failure/report redaction, local-unverified subject distinction, package selection/extraction and unchanged exact verified artifact checks |

## Physical package and source-HEAD closure

[#415/#416](https://github.com/Harness-X-Harness/codex/issues/416) move all nine
real owner implementations. [#417](https://github.com/Harness-X-Harness/codex/issues/417)
moves backend Facts. [#418](https://github.com/Harness-X-Harness/codex/issues/418)
binds package-qualified owner checks and layout controls to those physical
packages. Shared catalog metadata drives both scenario names and tagged Go
owner names; no separate workflow scenario list is authoritative. Default
`./...` remains credential-free and excludes all executable/backend entry points.

[#419](https://github.com/Harness-X-Harness/codex/issues/419) makes affected native
checks reachable on actual feature-push HEADs, reusing the existing native proof
and one offline Go invocation. Subject validation checks event/ref/workflow/checkout
identity; PR synthetic merge and actual feature HEAD remain distinct. Generated
schema consistency, product Rust tests and real-binary Integration still need
executed exact-source evidence. A YAML check, old binary or Go mock cannot supply
that acceptance. Current paid Live/Facts and CI results are not claimed here.

## Fresh measurements and limits

The initial `7b5a2054e7a763d7c6b833d9adad8020557dc7b5` source could not pass
`go list`: embedded `grok/dist` assets were absent. The fresh runnable baseline
is the assets-only restoration commit
`6a782e6fc55c86f6fc5e699d589aed33b4b5aef1`, tested in a clean separate checkout.
It is not yesterday's run or a current-head proof.

The rebuilt non-race measurements below are from a fresh clean checkout of
`f029fd50525dc98e6c9dc6e88ea30d3eca5629ac`. An earlier clean checkpoint,
`e063b64f37557e91178537057d3eace7a40cf54f`, has a separate race result; it must
not be relabeled as the later source's race proof.

| Fresh local control | Baseline | Rebuilt result |
| --- | --- | --- |
| Default `go test -count=1 -timeout=15m -json -coverprofile=... ./...` | 949 named test invocations: 934 pass, 15 skip, 0 fail; `live` 62.448 s | At `f029fd5`: 1,055 run/pass, 0 skip, 0 fail; `live` 72.479 s |
| Aggregate statement coverage | 81.3% | 82.0% at `f029fd5` |
| JSON completion and both required offline owners | Baseline had implicit skipped external tiers | Passed at `f029fd5`; omission controls separately reject either missing required owner |
| `go list`, format, `go vet`, `go mod tidy -diff`, clean checkout | Passed at assets-only baseline | Passed at `f029fd5`, including tagged vet/format controls |
| Default `-race` | Baseline command above was not `-race` | At exact `f029fd5`: 1,055 run/pass, 0 skip/fail, required-owner checker passed, `live` 447.285 s. Earlier `e063b64`: 1,052 pass, `live` 437.989 s |
| Five Python packaging test files | No result inferred from Go | 57 passed at clean `e063b64`; Python sources unchanged at `f029fd5` |
| Actual-binary tagged Integration | No native binary result inferred | Not executed by this local audit; missing-input failure controls do not prove positive composition; original-wire X compatibility remains required |
| Paid Live/Facts and Actions | Not executed by this local audit | Not inferred from local tests or YAML validation |

The `f029fd5` checkpoint adds three deterministic Live preflight cases beyond
`e063b64`; both the later whole-module non-race and race runs passed at the
exact `f029fd5` source. Recorded local timings overlap other test processes, so
they do not establish a speedup or regression. The structural change removes duplicated offline execution from the
runtime job; actual Actions runtime and backend billing effects remain unmeasured.

At the tested `f029fd5` checkpoint, executable digest verification checks the
selected path before launch and after cleanup. It assumes the executable and containing directory remain
trusted and unchanged; replace/execute/restore between checks can evade it.
The stronger continuous executed-byte stability acceptance in
[#404](https://github.com/Harness-X-Harness/codex/issues/404) is explicitly unmet.
A positive pre/post digest control must not be reported as proof of that stronger
property, and no native launcher/helper-preserving fix is validated here.

The 15 baseline skipped events comprised two backend parents and 13 native
parent/descendant skips. Removing implicit default skips changes counts without
claiming their external behavior passed. The rebuilt suite also adds local
consumer/selection/event-checker controls. Compare boundaries and assertions,
not total counts alone. No runtime speedup, billing reduction, current native
pass or current Live success is claimed without measured execution evidence.
