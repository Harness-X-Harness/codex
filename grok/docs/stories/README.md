# Grok Stories

User-visible Grok claims. Each Story names one product boundary and the native
Rust or Go test that binds it.

Deterministic Stories are proven by Cargo tests in this repository. Live
Stories are proven by `ronhuafeng/llm-go` `codexsdk` `go test -run '^TestGrok'`
against the exact Linux archive. A GREEN run of that Go suite is Live evidence.
Changing an outcome-driving acceptance requires changing the named test.

These files are not executable acceptance input. Do not add tests that validate
this directory's existence, headings, or filenames.

## Live composition

| Story | Proof |
|-------|-------|
| [Grok artifact starts with the supported Provider profile](./grok-provider-profile-startup.md) | `TestGrokBasic` |
| [Encrypted reasoning survives full-history continuation](./grok-encrypted-reasoning-history-continuation.md) | `TestGrokEncryptedReasoningContinuation` |
| [Child agent inherits parent Provider authority](./grok-provider-binding-lifecycle.md) | `TestGrokCollaboration` |
| [Image generation and same-Thread history edit](./grok-image-generation-history-edit.md) | `TestGrokImageGenerationEdit` |
| [Custom `apply_patch` file edit](./grok-custom-apply-patch.md) | `TestGrokCustomApplyPatch` |

## Deterministic App Server

| Story | Proof |
|-------|-------|
| [One process serves exactly the bundled Grok catalog](./provider-catalog-app-visibility.md) | `codex-rs/app-server/tests/suite/v2/grok_model_list.rs` |
| [Provider binding across Thread fork](./grok-provider-bound-thread-fork.md) | `codex-rs/app-server/tests/suite/v2/grok_provider_binding.rs` |
| [Provider binding across cold restart and resume](./grok-provider-bound-cold-restart-resume.md) | `codex-rs/app-server/tests/suite/v2/grok_provider_binding.rs` |
| [Provider binding across compaction and continuation](./grok-provider-bound-compaction-continuation.md) | `codex-rs/app-server/tests/suite/v2/grok_provider_binding.rs` |

Architecture: [`../architecture.md`](../architecture.md).
