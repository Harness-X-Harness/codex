# Grok release

This document is the delivery Northstar. Markdown here is not executable
acceptance input. Product semantics live in [`architecture.md`](./architecture.md).

## Northstar

```text
PR to grok/rust-v*      = Cargo
push grok/rust-v*       = six target binaries + Live on Linux musl
grok/release.py publish = the only grok-v* mutation
```

A failed or cancelled proof does not replace the channel.
The workflow is a proof graph, not a publication program.
`work/*` is not the Codex tree.

```text
commit SHA      = immutable source identity
Actions run     = proof of six target binaries and Linux Live
grok-vX.Y.Z     = moving channel written by grok/release.py publish
```

## Proof

A pull request to `grok/rust-v*` runs Cargo. It does not build binaries or
run Live. Push does not repeat Cargo.

```text
push grok/rust-vX.Y.Z
  -> build x86_64-unknown-linux-musl
  -> Go Live on that binary
  -> build the other five targets in parallel with Live
```

Live consumes the musl `codex` binary from the same run. It does not wait
for Darwin or Windows. Publication packages the six binaries.

GitHub Actions does not create or replace `grok-vX.Y.Z`.

## Publication

After a proof run is GREEN, from a checkout of that SHA:

```text
python3 grok/release.py publish --run-id RUN --repo OWNER/NAME
```

The publisher refuses unless the run is a successful `grok` push, Live
succeeded, each `TARGETS` artifact is present, and both the branch head and
the checkout HEAD equal the run SHA. It then replaces `grok-vX.Y.Z` and reads
back tag SHA, release target, asset names, and SHA-256 digests. A failed or
cancelled proof does not replace the channel. The publisher does not retry a
mutation that did not read back.

When work moves to a new stock tag, stop pushing the old `grok/rust-v*` line.
The old channel stops moving because nothing publishes it.
Do not publish `grok-v0.153.4`; that channel is frozen.

## Proof authorities

```text
cargo fmt/clippy/test     -> PR gate
cargo build               -> six target binaries
go test -run '^TestGrok'  -> Live on the Linux musl binary
GitHub Actions            -> proof orchestration and artifacts
grok/release.py publish   -> package, channel mutation, readback
```

Workflow mechanics live in [`.github/workflows/grok.yml`](../../.github/workflows/grok.yml).
Publication lives in [`grok/release.py`](../release.py).
Stock-tag adoption lives in [`carry-forward.md`](./carry-forward.md).
