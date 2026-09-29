# Grok Stories

User-visible Grok claims. Each Story names one product boundary and the native
Rust or Go test that binds it.

Deterministic Stories are proven by Cargo tests in this repository. Live
Stories are proven by `grok/live` `go test -run '^TestGrok'` against the
configured Grok binary. `llm-go/codexsdk` is the SDK, not the acceptance
owner. A GREEN run is Live evidence. Changing an outcome-driving acceptance
requires changing the named test. Scheduling is orchestration and does not
change semantic authority.

These files are not executable acceptance input. Do not add tests that validate
this directory's existence, headings, or filenames.

A Live Story's "Partial success is not completion" list is the ordered stage
list of its test: each entry corresponds to one `failStage` name
(`thread_bound_to_grok`, `turn_completed`, …) that a RED run prints as
`stage=`. Stage names are stable identifiers; renaming one changes the Story,
so both move in the same commit.
Triage a RED by its `stage=` value and the owning semantic seam. Do not infer
product authority from the workflow or artifact context in which the test ran.

## Live composition

| Story | Proof |
|-------|-------|
| [Grok artifact starts with the supported Provider profile](./grok-provider-profile-startup.md) | `TestGrokBasic` |
| [Encrypted reasoning survives full-history continuation](./grok-encrypted-reasoning-history-continuation.md) | `TestGrokEncryptedReasoningContinuation` |
| [Child agent inherits parent Provider authority](./grok-provider-binding-lifecycle.md) | `TestGrokCollaboration` |
| [Image generation and same-Thread history edit](./grok-image-generation-history-edit.md) | `TestGrokImageGenerationEdit` |
| [Structured exact-match file edit](./grok-structured-edit.md) | `TestGrokStructuredEditWireContract`, `TestGrokStructuredEdit`, `TestGrokStructuredEditApprovalDeclined`; optional `TestGrokStructuredEditPinnedPreviousModel`, `TestGrokStructuredEditReplaceAll` |
| [Hosted `web_search` Turn and replay](./grok-hosted-web-search-turn.md) | `TestGrokHostedWebSearch` |
| [Hosted `web_search` domain allowlist](./grok-hosted-web-search-allowlist.md) | `TestGrokHostedWebSearchAllowlist` |
| [Hosted `web_search` domain blocklist](./grok-hosted-web-search-excluded-domains.md) | `TestGrokHostedWebSearchExcludedDomains` |
| [Hosted `x_search` Turn and follow-up replay](./grok-hosted-x-search-turn.md) | `TestGrokHostedXSearch` |
| [Hosted `x_search` date window](./grok-hosted-x-search-date-window.md) | `TestGrokHostedXSearchDateWindow` |

## Deterministic App Server

| Story | Proof |
|-------|-------|
| [One process serves exactly the bundled Grok catalog](./provider-catalog-app-visibility.md) | `codex-rs/app-server/tests/suite/v2/grok_model_list.rs` |
| [Provider binding across Thread fork](./grok-provider-bound-thread-fork.md) | `codex-rs/app-server/tests/suite/v2/grok_provider_binding.rs` |
| [Provider binding across cold restart and resume](./grok-provider-bound-cold-restart-resume.md) | `codex-rs/app-server/tests/suite/v2/grok_provider_binding.rs` |
| [Provider binding across compaction and continuation](./grok-provider-bound-compaction-continuation.md) | `codex-rs/app-server/tests/suite/v2/grok_provider_binding.rs` |

Architecture: [`../architecture.md`](../architecture.md).
