# Grok release

This document is the human-readable Grok delivery design. Executable
correctness is owned by native Cargo tests and builds, direct `go test` Live
on the exact Linux archive, the Actions run that wires those artifacts, and
one minimum GitHub publication readback.

```text
commit SHA      = immutable source identity
Actions run     = execution context
grok-vX.Y.Z     = latest GREEN distribution channel for stock rust-vX.Y.Z
```

## Normal path

```text
push grok/rust-vX.Y.Z
  -> actionlint
  -> direct Rust native checks and required target builds
  -> Go native Live on the exact Linux artifact from that run
  -> publish or replace grok-vX.Y.Z
  -> one authoritative GitHub readback
```

A pull request to `grok/*` runs `actionlint` and the Rust native checks. It
does not publish.

An existing `grok-vX.Y.Z` channel is normal state. A failed candidate does not
replace it. A later GREEN push on the same stock line replaces the channel.
When work moves to a new stock tag, the old channel stops moving because
nothing pushes it; there is no freeze operation.

Git owns source identity as the commit SHA. The published channel is a moving
pointer to the latest GREEN SHA for that stock version. Same-name tag
immutability, destination emptiness, one publication request ever, and rebuild
suffixes such as `-1` / `-2` are not part of this model.

Publication starts only after required candidate, artifact, and Live proof is
GREEN. There is no separate human publication decision after that proof.

## Proof authorities

```text
actionlint          -> GitHub Actions DSL static checking
cargo fmt/clippy    -> Rust native formatting/linting
cargo test          -> Rust deterministic product semantics
cargo build         -> supported release targets
go test -run '^TestGrok' -> real Grok composition on the exact Linux archive
GitHub Actions      -> orchestration and artifact flow
GitHub readback     -> publication external-effect confirmation
```

Markdown in this file is not executable acceptance input. Do not add tests,
scripts, greps, schemas, Story inventories, heading/keyword checks, or
required manual-review checklists to enforce it.

Workflow mechanics that implement this model are owned by Codex #167–#170.
Stock-tag adoption is owned by [`carry-forward.md`](./carry-forward.md) when
that document exists. Sibling-branch topology is
[`docs/downstream-products.md`](../../docs/downstream-products.md).
