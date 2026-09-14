# Grok release

This document is the human-readable Grok delivery design. Markdown here is
not executable acceptance input.

```text
commit SHA      = immutable source identity
Actions run     = proof of tests, six target archives, and Linux Live
grok-vX.Y.Z     = moving channel written by the external publisher
```

## Proof

```text
push grok/rust-vX.Y.Z
  -> cargo fmt / clippy / tests
  -> build x86_64-unknown-linux-musl
  -> Go Live on that archive
  -> build the other five targets in parallel with Live
```

A pull request to `grok/**` runs only the Cargo checks.

Live consumes the `x86_64-unknown-linux-musl` archive from the same run.
It does not wait for Darwin or Windows. Publication still requires all six
archives.

GitHub Actions does not create or replace `grok-vX.Y.Z`.

## Publication

After a proof run is GREEN, from a checkout of that SHA:

```text
python3 grok/publish.py --run-id RUN --repo OWNER/NAME
```

The publisher refuses unless:

- the run is the `grok` workflow on a `push` to `grok/rust-v*`
- the run completed successfully
- Live and all six build jobs succeeded
- `heads/grok/rust-vX.Y.Z` equals the run SHA
- the checkout HEAD equals the run SHA

It then replaces `grok-vX.Y.Z` and reads back tag SHA, release target, asset
names, and SHA-256 digests. A failed or cancelled proof does not replace the
channel. The publisher does not retry a mutation that did not read back.

When work moves to a new stock tag, stop pushing the old `grok/rust-v*` line.
The old channel stops moving because nothing publishes it.

## Proof authorities

```text
cargo fmt/clippy    -> Rust native formatting/linting
cargo test          -> Rust deterministic product semantics
cargo build         -> six release targets
go test -run '^TestGrok' -> real Grok composition on the Linux archive
GitHub Actions      -> proof orchestration and artifacts
grok/publish.py     -> channel mutation and GitHub readback
```

Workflow mechanics live in [`.github/workflows/grok.yml`](../../.github/workflows/grok.yml).
The publisher lives in [`grok/publish.py`](../publish.py).
Stock-tag adoption lives in [`carry-forward.md`](./carry-forward.md).
Sibling branch topology lives in
[`docs/downstream-products.md`](../../docs/downstream-products.md).
