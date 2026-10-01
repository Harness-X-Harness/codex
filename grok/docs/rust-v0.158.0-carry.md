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
| C2 / #323 | Atomic close/snapshot and durable post-close input disposition. | C0. | Queue close/snapshot and post-close injection/history integration, plus affected stock session/turn/mailbox/AgentControl regressions and lifecycle composition. |
| C3 / #327 | Exact whole-number parsing and current tool call-site wiring. | C0. | Signed/unsigned bounds, integral decimal/exponent forms, fractions, values above 2^53, and exec/stdin/multi-agent argument integration. |
| C4 / #321 | Structured editing with native local/remote mutation safety and retained scenario source. | C0; C1 harness for Live scenario source. | Engine/runtime/lifecycle, stale snapshots, VerifiedContents, registration/hooks, conditional writes, stock patch/Code Mode/sandbox/symlink/hard-link/cwd regressions, and deterministic harness tests. |
| C5 / #328 | Updated Grok image dialect and retained image scenario source. | C1. | Projection/normalization/cardinality and fail-closed input policy; stock transparent-background/file-backed edit regressions; availability/schema/request composition and deterministic image harness tests. |
| C6 / #329 | Runtime profile/catalog and Provider-bound App Server lifecycle. | C1. | Asset loading, model-list/catalog isolation, Provider-bound fork/resume/compaction/child inheritance, stock/non-Grok catalog and lifecycle regressions, and deterministic shipped-profile harness tests. |
| C7 / #322 | Complete tool/search/history contract, owned outputs, and scenario source. | C1; other owners only where actual seams require them. | Collision/reverse routing/function/custom/patch grammar, hosted history/follow-up versus local output pairing, search/date precedence and fail-closed policy; affected stock planning/routing/serialization/history regressions; owned consistency, deterministic scenarios, and cross-owner composition. |
| C8 / #338 | Independently invocable backend Facts. | C0 and C1 Facts foundation. | New invocation/configuration/result-handling tests and affected composition; event/permission/credential prerequisite and actual invocation verification; backend results remain separate. |
| C8 / #339 | Complete package-to-Live invocation and branch-local operations contract. | C0/C1 and relevant #321/#328/#329/#322 assets/scenarios. | New staging/orchestration tests, complete same-run package inputs, explicit artifact subject validation, and actual entrypoint verification at the claimed level. |

### C1 serial boundaries

#320 is split into complete increments: C1a exposes an explicit, typed
Basic/reasoning HTTP dialect with text/history whitelist, bounded interleaved
SSE normalization, and fail-closed unsupported input. C1b maps serialized
`grok_responses` to that API in production, closes Provider defaults and lossless
remote-config/schema boundaries, and proves runtime composition. C1c introduces
actually used Facts/Live Basic/Provider/reasoning support and deterministic Go
proof, with explicit harness model fixtures independent of #329 and accepted
Story/gate reviews. #320 stays open until all three are complete.

C1a's public client is useful independently of runtime configuration. It rejects
tools/images and raw JSON in its bounded Grok surface; #322/#328 retain extension
ownership. Combining request projection and ingress sequencing in C1a avoids
advertising a half-working dialect. Each stage must obtain its own PR and actual
canonical-push proof before the next begins; no obligation is retired.

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

The inner `complete` job explicitly needs `subject` and native `locks`;
outer `Cargo` needs the reusable `proof`. Both aggregates run with `always()`
and accept only `success`. The lock job requires successful subject verification
and uses native Cargo/Bazel entrypoints plus the existing Provider metadata suite.
The same native job runs the complete codex-api suite and scoped Clippy for
Grok deltas and affected stock request/ingress/error contracts, sharing setup and
compiled dependencies. It receives no backend credentials. Sequential required
steps preserve fail-closed aggregation; C0 topology is unchanged.
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
