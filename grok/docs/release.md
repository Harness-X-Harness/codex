# Grok release

This document is the delivery Northstar. Markdown here is not executable
acceptance input. Product semantics live in [`architecture.md`](./architecture.md).

## Northstar

```text
PR to grok/rust-v*      = Cargo
push grok/rust-v*       = six target binaries + Live on Linux musl
grok/release.py publish = the only grok-v* mutation
```

```text
commit SHA      = immutable source identity
Actions run     = proof of six target binaries and Linux Live
grok-vX.Y.Z     = moving channel written by grok/release.py publish
```

## Rules

A later change that breaks a rule is a regression. Instances (temporary
branches, frozen channels, one failed run) stay in operational notes.

1. Heavy work happens once per SHA. Later stages consume the result.
2. Review, proof, and publish are the only entries. Do not add a process
   that proves the proof.
3. Evidence is the named executable result. Do not add ledgers, document
   validators, or identity jobs.
4. One fact has one authority.
5. Prefer native platform steps. Scripts wrap only external mutation and
   readback.
6. Add a layer only when it prevents a current failure. Cache, sibling
   cancel, single-target retry, and pack/unpack on the proof path are not
   rules.
7. A failed or cancelled proof does not replace the channel. Do not retry
   a mutation that did not read back.
8. The product line is the current stock fixed point. Temporary branches
   and old lines are not publication authorities.
9. Composition proof runs against the built binary. Packaging belongs to
   publish.
10. Capability text follows current code. Delete dead paths with evidence;
    do not infer legacy from names or history.

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

Both events run only when a proof input changes. Every path is a proof
input unless the workflow's `paths` filter negates it; the negated set is
text that nothing compiles, packages, or executes (repository docs, root
Markdown, editor and agent configuration, other workflows). Live Stories,
`grok/dist`, `LICENSE`, `grok/release.py`, Markdown under `codex-rs`, and
the workflow itself are proof inputs. The filter in `grok.yml` is the only
authority for that set; the publisher reads it rather than repeating it.

GitHub Actions does not create or replace `grok-vX.Y.Z`.

## Publication

After a proof run is GREEN, from a checkout of the branch head:

```text
python3 grok/release.py publish --run-id RUN --repo OWNER/NAME
```

The publisher refuses unless the run is a successful `grok` push, Live
succeeded, each `TARGETS` artifact is present, the checkout HEAD is the
branch head, and that head is the run SHA or descends from it through
commits that change no proof input under the workflow's `on.push.paths`
filter. The tag and the release target stay at the run SHA, the commit the
binaries were built from. It then replaces `grok-vX.Y.Z` and reads back tag
SHA, release target, asset names, and SHA-256 digests. A failed or cancelled
proof does not replace the channel. The publisher does not retry a mutation
that did not read back.

When work moves to a new stock tag, stop pushing the old `grok/rust-v*` line.
The old channel stops moving because nothing publishes it.
The `grok-v0.153.4` channel is frozen.

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
