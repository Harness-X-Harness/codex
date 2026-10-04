# Grok 0.158 development contract

This version plan is owned by [#333](https://github.com/Harness-X-Harness/codex/issues/333).
Current development policy is
[grok/main:carry-forward.md](https://github.com/Harness-X-Harness/codex/blob/grok/main/grok/docs/carry-forward.md);
review and runtime-evidence policy are its linked documents. Dynamic results and
scheduling belong to the owning GitHub issues and PRs.

## Pinned inputs

- Exact stock: upstream `openai/codex` annotated tag `rust-v0.158.0`, peeled commit
  `064c6b8c737f5b41d171fdda80bd9ef10ad06eb3`.
- Previous accepted product reference: `grok/rust-v0.157.1` at
  `b187b716787cf078a4c0f51986eb3555e9f6c6de`. It remains the accepted baseline
  throughout initialization until the new line's complete product/process review.
- Canonical development line: `grok/rust-v0.158.0`, initialized at exact stock
  through [#319](https://github.com/Harness-X-Harness/codex/issues/319).
- Old investigation: `efc0dddb4ae8aad006910a27d0420b5b7002287a`, retained in verified
  `archive/grok-rust-v0.158.0-pre-reset-20261001` and
  `archive/grok-rust-v0.158.0-rc-pre-reset-20261001` refs. Its implementations,
  tests, and failure history are reference evidence, not current proof. The old
  carry ref remains historical; the old RC source ref is retired.

Exact stock is a trusted upstream input. C0 establishes this contract and the
admission/proof mechanism. Later increments own their deltas, affected stock
contracts (including indirect effects), and composition. Neither stock trust nor
C0 acceptance proves the modified Grok product.

## Selected decisions and current-stock ownership

KEEP retains the requested behavior; UPDATE adapts its implementation or selected
capability at stock seams. Each owner must check actual stock ownership before
changing code and use stock when it already satisfies the contract.

| Decision | Retained responsibility and reason | Owner / current seam |
| --- | --- | --- |
| KEEP / UPDATE | Explicit Provider identity and `grok_responses` dialect; destination and display name do not select semantics. Adapt request/history/SSE projection and recovery defaults; reject unsupported remote conversion. | #320: Provider/API/config owners; stock concrete transport is in `codex-client`. |
| KEEP | Atomic turn-input close/snapshot and durable post-close disposition, independently of Provider identity. | #323: stock session, AgentControl, and mailbox boundaries. |
| KEEP | Exact, in-range mathematically integral JSON tool arguments without an `f64` round trip. | #327: current integer-valued tool argument call sites. |
| KEEP / UPDATE | Structured exact-match conditional writes and stale-content rejection, composed with stock mutation safety. | #321: stock patch, runtime, hooks, and local/remote filesystem owners. |
| UPDATE | Grok generation/edit projection at the moved image seam; preserve stock transparency and file-backed editing. Unsupported Grok representations remain fail-closed. | #328: stock image API and extension owners; backend support remains not rerun. |
| KEEP / UPDATE | Shipped Grok profile/catalog policy and Provider-bound lifecycle through stock configuration, fork/resume/compaction, and child inheritance. | #329: profile/catalog source and App Server/session owners. |
| KEEP / UPDATE | Collision-safe flat projection, reverse dispatch, explicit hosted replay, search/date policy, and wire whitelist at the Provider boundary. | #322: stock planning, routing, history, configuration, and API owners. |
| UPDATE | Native PR/post-squash proof, independently callable Facts, package-backed Live, and complete packaging capability. These mechanisms must exist at their claimed invocation level. | #337, #338, #339; common harness mechanics enter only when first used by #320. |
| DROP | Old carry caller, owner-marker detection, stage manifests, progressive/full modes, long-lived RC source, bulk promotion, and historical product-state machinery. | #319/#337 replace the old process with native version-line development. |

## Intended increments and minimum proof

The table records the selected serial landing order. Dependencies identify actual
inputs, rather than requiring every earlier row as a semantic prerequisite.

| Position / issue | Complete outcome | Real prerequisites | Minimum native proof |
| --- | --- | --- | --- |
| C0 / #337 | This plan and one demonstrated native admission/proof path. | #319 readiness. | Execution-subject checks; real PR/required `Cargo` and protection evidence; fail-closed aggregation including an executed negative path; event/permission/concurrency review; final-candidate PR and actual canonical-push proof. |
| Before C1 / #343 | Manifest-aligned workspace lock identity for reproducible Provider proof. | C0; first needed by C1. | Native locked Cargo resolution, Cargo-owned Bazel lock generation/consistency, non-empty Provider metadata tests, reviewed aggregation and PR/canonical proof. No stock product/fixture changes. |
| C1 / #320 | Provider/Responses, owned representations, and first used Facts/Live support. | C0. | Identity/dialect, request/history policy, interleaved SSE, recovery/remote boundaries; affected OpenAI/ChatGPT serialization, ingress/error/config regressions and composition; meaningful deterministic Go harness tests. |
| C2 / #323 | Atomic close/snapshot and durable post-close input disposition. | C0. | Queue close/snapshot and post-close injection/history integration through the real completion window, plus affected stock session/turn/mailbox/AgentControl, public injection/steering and goal-advice contracts. |
| After C2: C1 correction / #320 | Keep compaction capability consistent with explicit Grok dialect under provider aliases. | C1; C2 is the selected landing order, not a semantic input. | Grok versus stock Responses capability controls, actual alias manual/automatic local summary and continuation, and affected stock local/V2/TokenBudget consumers. |
| Before C3a: stock host-fixture prerequisite / #327 | Align the existing interrupt fixture with the current ToolDefinition so the consumed host integration target compiles. | C0; demonstrated consumer C3a. | Pinned stock V8 setup, exact host build, separately nonempty existing interrupt regression, scoped host format/lint, and all prior applicable proof. |
| C3a / #327 | Exact whole-number exec arguments and their producer paths. | Stock host-fixture prerequisite. | Complete private helper, exact bounds/forms and field wiring, real exec effects, Bash/Code Mode and standalone stdio/gRPC precision composition, and owned feature/lock closure. |
| C3b / #327 | Exact stdin arguments and their trace consumers. | C3a's complete shared helper. | Real stdin delivery/rejection and returned-ID composition, exact trace limit parsing, canonical numeric session keys, reducer replay and affected stock behavior. |
| C3c / #327 | Exact wait/fork consumers and agent composition. | The same helper from C3a/C3b. | Legacy/v2 wait policy, numeric fork with retained string modes, actual mailbox/history selection, no-child rejection, generic updatedInput hooks and affected stock behavior. |
| Before C4: native proof construction / #333 | Early source normalization, profile-aligned schema generation, and bounded build measurements. | Accepted C3c; existing native proof and ordinary commit path. | Preserve formatter/fixer scopes and warning-level normalization, reject changed input before behavioral validation, inspect bound failure patches, verify schema bytes and unchanged strict/native coverage, then exact PR and canonical proof. Metrics are diagnostic only. |
| C4a / #321 | Restore the callable public conditional filesystem API. | C0; accepted pre-C4 native proof construction. | Typed/wire/client/router/handler closure; local/remote selected sandbox and follow/no-follow behavior; shared effect ownership through cancellation/disconnect; contention, helper settlement, lost-reply no-resend and affected stock filesystem/link/cwd/patch proof on Linux. |
| C4b / #321 | Restore the complete structured editor and retained scenarios. | C4a's complete conditional mutation contract; C1 harness for scenario source. | Exact matching and VerifiedContents, tool/model registration, runtime/approval/hook identity, committed deltas, owned representations, real local/remote and Code Mode composition, and deterministic retained scenario support. |
| C5 / #328 | Updated Grok image dialect and retained image scenario source. | C1. | Projection/normalization/cardinality and fail-closed input policy; stock transparent-background/file-backed edit regressions; availability/schema/request composition and deterministic image harness tests. |
| C6 / #329 | Runtime profile/catalog and Provider-bound App Server lifecycle. | C1. | Asset loading, model-list/catalog isolation, Provider-bound fork/resume/compaction/child inheritance, stock/non-Grok catalog and lifecycle regressions, and deterministic shipped-profile harness tests. |
| C7 / #322 | Complete tool/search/history contract, owned outputs, and scenario source. | C1; other owners only where actual seams require them. | Collision/reverse routing/function/custom/patch grammar, hosted history/follow-up versus local output pairing, search/date precedence and fail-closed policy; affected stock planning/routing/serialization/history regressions; owned consistency, deterministic scenarios, and cross-owner composition. |
| C8 / #338 | Independently invocable backend Facts. | C0 and C1 Facts foundation. | New invocation/configuration/result-handling tests and affected composition; event/permission/credential prerequisite and actual invocation verification; backend results remain separate. |
| C8 / #339 | Complete package-to-Live invocation and branch-local operations contract. | C0/C1 and relevant #321/#328/#329/#322 assets/scenarios. | New staging/orchestration tests, complete same-run package inputs, explicit artifact subject validation, and actual entrypoint verification at the claimed level. |

#320/#333 detail serial C1a (typed Basic/reasoning HTTP + bounded SSE), C1b
(runtime/config closure), and C1c (used Facts/Live) increments. C1c first lands
C1c1 (Basic/pinned/reasoning Facts and deterministic HTTP proof), then C1c2
(consumed Live runner and scenarios with deterministic App Server proof):
C1c2a closes Basic binary/process/fixture observation, including real App Server
and controlled HTTP composition; C1c2b adds the shared-runner reasoning/tool/history
scenario and deterministic public-stdio proof. Actual native tool/HTTP composition
and real-provider reasoning continuation require #322's tool support; C1c2b alone
cannot establish that composition or a real Story result.
Each uses its own PR/squash/canonical proof. #320 stays open until all close.
Harness fixtures are independent
of #329; #322/#328 retain Tools/Images ownership; no obligation is retired.

The scoped C1 correction after C2 closes a discovered capability mismatch:
OpenAI/Azure name or URL heuristics must not select remote V2 compaction for the
explicit Grok dialect, whose request projection does not support its trigger.
Grok retains the existing local compactor; stock Responses capability rules and
all unrelated capabilities remain unchanged. This correction has its own
reviewed PR/squash/canonical proof before C3. Prior successful executions remain
historical evidence; no new backend or universal compaction claim is introduced.

The separately named stock host-fixture prerequisite supplies the existing
interrupt fixture's omitted `input_schema_max_bytes: None` and the first
consumed host proof inputs. The field has no effect when that fixture's input
schema is absent. Its independent reviewed PR/squash/canonical loop precedes
C3a; the numeric increments inherit the accepted setup and regression once.
No numeric parser, tool behavior, V8 version/feature or protocol shape is changed
by this prerequisite.

C3 uses three independently useful increments under the same #327 owner. C3a
completes the private helper and exec paths with their producer and native effect
proof. C3b pairs stdin admission with its trace reducer: exact limit parsing and
canonical numeric session keys. Exec trace projection uses normalized runtime
fields; only stdin reparses these original numeric arguments. When that second
consumer needs it, move the same helper to the existing protocol dependency.
C3c adds wait/fork and agent composition. No unused future adapter or duplicate
parser is introduced. Each increment closes its source, owned outputs and proof
through the ordinary reviewed PR/squash/canonical loop before the next starts.
#327 closes only after all three; no numeric or earlier applicable proof
obligation is retired.

C4a restores a previously shipped public execution capability, not an inert trait
or a model-tool claim. Its mutation owner covers compare/write and participating
ordinary write/remove/copy effects through actual settlement; helper uncertainty
cannot silently release admission. Unrelated writers, kernel CAS, rollback and
crash durability remain outside its contract. C4b keeps the complete editor and
retained scenarios together; no C4c or safety/proof deferral is planned. #321 stays
open through both outcomes, each with reviewed PR/squash and actual canonical proof.

The retained [structured-edit Story](./stories/grok-structured-edit.md) owns the
four callable scenarios, packaged-Grok acceptance boundary and unproven downstream
obligations. C4b's Go tests establish deterministic support; its actual editor
composition proof is provider-neutral Rust on Linux.

The owner narrowed this carry-forward roadmap's required native verification to
Linux on 2026-10-04. This scope applies to C4a, C4b and subsequent increments;
platform-neutral proof obligations in the table mean Linux. Distribution targets
remain separately specified below. C4a proof covers the actual Linux local/remote
filesystem, cancellation, helper settlement and affected stock contracts. Windows
verification is outside this plan and its job is removed from the required
aggregate. Platform implementations and existing regression tests remain available
without a Windows acceptance claim. Earlier Windows failures and bounded
diagnostic results remain historical observations; this scope change does not
turn them into passing evidence.

An owner may split only at independently complete green boundaries, updating this
plan and #333 together. Each semantic owner closes its affected native generator
outputs, schemas/SDKs, lockfiles, Bazel/build data, fixtures, tests, and harness
pieces in the same increment. C7 closes accumulated behavioral composition;
process integration owns new orchestration proof rather than deferred semantics.
Retain earlier still-applicable downstream obligations; reviewed test replacements
must provide equivalent or stronger coverage. Inspect actual impact through
callers, shared dependencies, configuration, and runtime boundaries, broadening
native proof when needed. A genuinely required stock adaptation gets a separately
named, scoped increment before its consumer, using the same development loop.

## Native admission/proof mechanism

`.github/workflows/grok.yml` calls the same-revision local `grok-proof.yml` for
both PR and version-line push events. The stable outer `Cargo` check requires the
whole reusable proof. C0's required `subject` job verifies the actual checkout
against the event SHA, PR merge parents or canonical push ref, and the caller's
workflow revision. Logs identify PR HEAD/base, checkout SHA and workflow context;
the PR test-merge SHA remains distinct from the eventual squash SHA.

The inner `complete` job explicitly needs `subject`, native `locks` and `runtime`,
and the consumed deterministic Go proof as it enters with C1c, all on Linux;
outer `Cargo` needs the reusable `proof`. Both aggregates run with `always()`
and accept only `success`. The lock job requires successful subject verification
and uses native Cargo/Bazel entrypoints plus the existing Provider metadata suite.
The same job adds the complete codex-api suite and scoped Clippy for Grok and
affected stock request/ingress/error contracts. Required steps preserve the
existing fail-closed aggregation and share compiled dependencies.
Missing, failed, cancelled, skipped, neutral, or empty required proof is non-green.
PR concurrency is scoped by PR number; canonical concurrency is isolated by run
ID, so even pending runs from distinct pushes cannot replace one another. The
callee has no competing concurrency group. Ordinary PRs have read-only contents
permission and no backend credentials.

C0 acceptance requires structural review, an applicable executed negative path,
actual required-check/protection evidence, fresh final-candidate PR proof and
same-definition proof on the actual protected squash SHA. Temporary failure
injection is removed before admission. These results stay in GitHub.

Later owners add delta, affected-stock and composition proof at their real seams,
with required native environment inputs and non-empty test selections. Keep
commands directly visible in this shared workflow and extend its explicit
required dependency graph. A canonical green prefix covers its still-applicable
downstream obligations; the original stock's baseline certification is upstream
responsibility. Stock failures retain their original evidence and are triaged
for actual downstream relevance. C0 contains no stock fixture/version/lock repairs.

### Construction before frozen validation

The existing scoped formatters and fixers run before behavioral validation.
Their machine-applicable warning suggestions remain part of normalization; an
unchanged nonfixing lint exit alone does not replace this check. Each construction
pass must leave a clean tracked and untracked worktree, or that input SHA fails.
The bounded failure patch identifies its checkout, PR head/base, run and tools.
Review its scope and semantics, then use the ordinary authorized commit path for
any correction. The new SHA must reach a fixed point and pass the full proof;
construction never grants acceptance or workflow write permissions.

Frozen validation preserves the nonfixing format and strict Clippy scopes,
nonempty behavioral selections, generated-output consistency and final clean-tree
checks. The schema recipe retains its normal default; CI selects the existing
ci-test profile with locked dependencies and compares generated bytes with HEAD.
Optional allowlisted resource observations and native Cargo timing reports inform
build optimization, not acceptance. No test result or compiled target is reused
across the required PR and actual canonical proofs.

## Evidence and delivery roles

- #324 owns backend claim classification and freshness. Historical, not-run, and
  unknown observations keep those labels; backend observations do not define the
  shipped catalog.
- #331 owns actual real-provider Live execution against an explicit binary and
  harness revision. Scenario source and invocation capability do not establish a
  successful backend run. Images retain UPDATE / not rerun until scoped evidence.
- #338 restores independent Facts invocation; #339 assembles complete packages
  and artifact-backed Live for the supported Linux musl x64 and macOS arm64
  targets. Backend credentials belong only to approved backend jobs.
- #325 owns optional requested downloads and package smoke. Same-run package
  inputs may support Live before separate download publication is completed.

Fresh backend evidence is parallel unless a specific product requirement or
discovered defect makes the relevant claim blocking. Product/process completion
and review against an actual green canonical revision establish the new accepted
product source in #333. Native green, branch existence, and artifact availability
each retain their narrower meaning.
