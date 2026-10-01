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
| C0 / #337 | This plan and one shared native PR/push baseline. | #319 readiness. | Repository formatting, stock CI-script tests, Cargo/Bazel lock consistency, Linux Rust workspace Clippy and all workspace tests; first required PR `Cargo` and actual squash push proof. |
| C1 / #320 | Provider/Responses, owned representations, and first used Facts/Live support. | C0. | Identity/dialect, request/history policy, interleaved SSE, recovery/remote boundaries, stock regressions, and meaningful deterministic Go harness tests. |
| C2 / #323 | Atomic close/snapshot and durable post-close input disposition. | C0. | Queue close/snapshot and post-close injection/history integration, plus stock session/turn regressions. |
| C3 / #327 | Exact whole-number parsing and current tool call-site wiring. | C0. | Signed/unsigned bounds, integral decimal/exponent forms, fractions, values above 2^53, and exec/stdin/multi-agent argument integration. |
| C4 / #321 | Structured editing with native local/remote mutation safety and retained scenario source. | C0; C1 harness for Live scenario source. | Engine/runtime/lifecycle, stale snapshots, VerifiedContents, registration/hooks, conditional writes, stock patch/Code Mode/sandbox/symlink/hard-link/cwd regressions, and deterministic harness tests. |
| C5 / #328 | Updated Grok image dialect and retained image scenario source. | C1. | Projection/normalization/cardinality and fail-closed input policy, stock image regressions, availability/schema agreement, and deterministic image harness tests. |
| C6 / #329 | Runtime profile/catalog and Provider-bound App Server lifecycle. | C1. | Asset loading, model-list/catalog isolation, Provider-bound fork/resume/compaction/child inheritance, stock regressions, and deterministic shipped-profile harness tests. |
| C7 / #322 | Complete tool/search/history contract, owned outputs, and scenario source. | C1; other owners only where actual seams require them. | Collision/reverse routing/function/custom/patch grammar, hosted history/follow-up versus local output pairing, search/date precedence and fail-closed policy, owned generators/consistency, deterministic scenarios, and cross-owner composition. |
| C8 / #338 | Independently invocable backend Facts. | C0 and C1 Facts foundation. | Existing deterministic harness proof plus invocation/event/permission/credential prerequisite verification; backend results remain separate. |
| C8 / #339 | Complete package-to-Live invocation and branch-local operations contract. | C0/C1 and relevant #321/#328/#329/#322 assets/scenarios. | New staging/orchestration tests, complete same-run package inputs, explicit artifact subject validation, and actual entrypoint verification at the claimed level. |

An owner may split only at independently complete green boundaries, updating this
plan and #333 together. Each semantic owner closes its affected native generator
outputs, schemas/SDKs, lockfiles, Bazel/build data, fixtures, tests, and harness
pieces in the same increment. C7 closes accumulated behavioral composition;
process integration owns new orchestration proof rather than deferred semantics.

## Executable native baseline

`.github/workflows/grok.yml` calls the same-revision local `grok-proof.yml` for
both PR and version-line push events. The stable outer `Cargo` check requires the
whole reusable proof; its inner aggregate requires every declared native job to
succeed, including after a failure or cancellation. Missing/skipped results fail.
PR runs may be superseded by a newer head; each canonical push retains its run.

C0's required baseline runs on Linux using stock CI/Bazel setup, pinned Rust/nextest,
the stock V8 artifact verifier, Linux sandbox dependencies, stock-built voice
SDK/runtime inputs, and built sandbox and Code Mode helpers. Commands remain directly visible: `just fmt-check`,
`just test-github-scripts`, `just bazel-lock-check`, workspace `just clippy`, helper `cargo build`, and
workspace `just test` with the stock `ci-test` build profile and explicit failure
on an empty test selection. The inherited stock platform workflows remain
stock-owned; C0 does not claim a completed macOS/Windows test run.

The stock voice helper requires GStreamer 1.28, beyond Ubuntu 24.04's default
packages. Its existing `//third_party/voice:native_runtime`, `native_sdk`, and
`pkg_config` targets prepare the pinned build and test inputs. Cargo keeps native
version probes and prioritizes that SDK's metadata, with explicit distro metadata
for ALSA. Tests use the matching libraries/plugins; this is source-test setup,
not a distribution package or an executed device/backend claim.

Stock stamps `workspace.package.version` as `0.158.0` while its checked-in lock
still records the workspace packages as `0.0.0`. C0 normalizes those lock entries
through native Cargo resolution so the locked baseline can execute. External
dependency versions, sources, checksums, and edges are preserved. The required
native Bazel lock refresh/check closes this baseline-owned representation.

The existing Code Mode cancellation fixture also needs the stock
`ToolDefinition::input_schema_max_bytes` field. C0 supplies `None`, matching its
absent input schema and the other stock fixtures. An unused matcher import in
the stock file-upload tests is removed for strict Clippy. These mechanical test
repairs preserve the existing assertions and product behavior.

Each later owner keeps this baseline and adds its applicable native/harness proof
to the same workflow and required dependency set. Real-backend opt-ins remain
outside this deterministic contract. Exact test selections must be non-empty.
Any stock failure is reported with its tested subject instead of being skipped.

Required PR proof, review, squash admission, and the actual canonical push result
are verified separately in GitHub. Progress waits for the exact-head result;
failures are retained and repaired/reverted through an ordinary PR. At a required
CI wait, the exact tested subject and authoritative result source remain explicit.

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
