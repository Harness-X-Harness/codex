# Grok delivery

This document owns the Grok delivery contract. Product semantics live in
[`architecture.md`](./architecture.md).

## Northstar

```text
PR to grok/rust-v*     = Cargo
push grok/rust-v*       = complete target distributions + Linux Live
workflow_dispatch       = Live only from an existing Linux distribution artifact
GitHub Actions artifact = delivery output for one exact commit SHA and target
```

GitHub Actions artifacts are the current delivery output.

Historical `grok-v*` GitHub releases remain historical outputs.

## Distribution artifact

Each target artifact is named:

```text
grok-<commit-sha>-<target>
```

The artifact is the complete per-target distribution. It contains:

```text
config.toml.example
models.json
INSTALL.md
LICENSE

bin/grok / bin/grok.ps1
bin/grok-bin / bin/grok-bin.exe
bin/codex-code-mode-host / bin/codex-code-mode-host.exe
bin/bwrap                         # Linux only
```

The distribution assets are readable without an installer. GitHub Actions
artifacts do not preserve Unix executable bits, so `INSTALL.md` includes the
small `chmod` step required after download. A human or agent then chooses a
dedicated product `CODEX_HOME` and does not reuse the normal `~/.codex` or
`~/.grok` Home.

## Rules

1. PR review proves Cargo formatting, lint, deterministic tests, and harness
   unit tests.
2. A push proof builds each shipped target once for that SHA.
3. Linux Live consumes the Linux distribution artifact from the same push run.
4. A failed or cancelled proof is a failed proof.
5. The commit SHA and Actions run are the source and proof identities.
6. The artifact from that run is the delivery output.
7. Facts remain independent backend evidence.
8. Installation is document-driven through `INSTALL.md`.

## Proof

A pull request to a candidate `grok/rust-v*` line runs Cargo on GitHub's
default pull-request merge ref. For this adoption the head is
`carry/grok-rust-v0.156.1` and the base is `grok/rust-v0.156.1`. It does not
build distribution binaries or run real-provider Live. `grok/main` remains the
latest validated product until a separate promotion.

A push to the candidate version line runs:

```text
x86_64-unknown-linux-musl
    -> complete Grok distribution artifact
    -> Grok Live on grok-bin from that artifact

aarch64-apple-darwin
    -> complete Grok distribution artifact
```

Current shipped targets are:

- `x86_64-unknown-linux-musl`
- `aarch64-apple-darwin`

Other targets stay out until they have users.

`workflow_dispatch` can run Live against an existing Linux distribution
artifact selected by `binary_run_id`. It is diagnostic proof only; it does not
create another delivery artifact.

Live runs `go test -v` directly. The Go harness owns failure diagnostics such as
`NOT_PROVEN`, stage names, and redacted wire evidence; the workflow does not
parse or reinterpret test results.

## Triage

| RED where | Read | Owner | Next |
|---|---|---|---|
| PR Cargo | failing step and native test | owning seam | fix the seam and its native test |
| target build | compiler/staging output | source or `.github/actions/build-grok` | fix and prove on a new PR/push |
| Grok Live | `NOT_PROVEN` stage and redacted wire evidence | capability, egress, ingress, or harness | fix the actual owner; do not add a blind retry |
| `TestFact*` | recorded vs observed class | corresponding whitelist row | update evidence and product behavior only when user-visible |

A GREEN build with a RED Live is not a completed Grok proof.

## Delivery

After a GREEN push proof, use the artifact from that exact run and target.
The artifact already contains the complete distribution.

## Proof authorities

```text
cargo fmt/clippy/test     -> PR gate
build-grok action         -> complete per-target distribution artifact
go test -run '^TestGrok'  -> Live on the Linux distribution artifact
GitHub Actions run        -> proof orchestration and immutable run context
INSTALL.md                -> human/agent installation procedure
```

Workflow mechanics live in
[`.github/workflows/grok.yml`](../../.github/workflows/grok.yml).
Target staging lives in
[`.github/actions/build-grok/action.yml`](../../.github/actions/build-grok/action.yml).
Stock adoption lives in [`carry-forward.md`](./carry-forward.md).
