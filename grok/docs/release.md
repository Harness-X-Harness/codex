# Grok delivery

This document owns the branch-local Grok proof and delivery contract. Product
semantics live in architecture.md.

Once carry-forward reaches RELEASE_HANDOFF, this document is the sole authority
for the remaining proof and delivery transitions on this version line.

## Northstar

~~~text
PR to grok/rust-v*
    = deterministic Cargo proof, including generated/precomputed closure

push grok/rust-v*
    = merged-PR provenance gate
    -> complete target distributions
    -> Linux Live on the exact Linux artifact

workflow_dispatch
    = diagnostic Live only from an existing Linux distribution artifact

GitHub Actions artifact
    = delivery output for one exact commit SHA and target
~~~

GitHub Actions artifacts are the current delivery output. Historical grok-v*
GitHub releases remain historical outputs.

## Distribution artifact

Each target artifact is named:

~~~text
grok-<commit-sha>-<target>
~~~

The artifact is the complete per-target distribution. It contains:

~~~text
config.toml.example
models.json
INSTALL.md
LICENSE

bin/grok / bin/grok.ps1
bin/grok-bin / bin/grok-bin.exe
bin/codex-code-mode-host / bin/codex-code-mode-host.exe
bin/bwrap                         # Linux only
~~~

The distribution assets are readable without an installer. GitHub Actions
artifacts do not preserve Unix executable bits, so INSTALL.md contains the
small chmod step required after download. A human or agent chooses a dedicated
product CODEX_HOME and does not reuse the normal ~/.codex or ~/.grok Home.

## Release states

This document proves these transitions after carry-forward handoff:

~~~text
RELEASE_HANDOFF
    -> DETERMINISTICALLY_PROVEN
    -> MERGED
    -> ARTIFACT_PROVEN
    -> SUCCESSFUL
~~~

DETERMINISTICALLY_PROVEN requires a green pull_request grok workflow with its
Cargo job successful. Local tests do not substitute for this transition.

MERGED requires the version-line head to be associated with a merged PR whose
base is this exact grok/rust-v* line and whose candidate received the green
Cargo proof.

ARTIFACT_PROVEN requires every shipped target to build for the exact merged
head and Grok Live to consume the Linux distribution artifact from that same
push run.

SUCCESSFUL requires every transition above to be satisfied. A direct push,
even if it compiles or could pass Live, is not a successful release transition.

## Rules

1. PR review owns deterministic proof: formatting, lint, native regression
   tests, harness unit tests, and stock-owned generated/precomputed consistency
   tests for the surfaces this product changes.
2. The Cargo job must include the App Server protocol schema/precomputed export
   consistency suite so checked-in schemas cannot diverge from embedded
   precomputed exports.
3. A version-line push must pass Release provenance before target builds start.
   The gate verifies that the pushed head is associated with a merged PR into
   the current version line and that the PR's grok workflow completed Cargo
   successfully.
4. A direct or otherwise unproven push must fail Release provenance. Build and
   Live success cannot retroactively replace missing PR proof.
5. After provenance passes, each shipped target builds exactly once for that
   SHA.
6. Linux Live consumes the Linux distribution artifact from the same push run.
7. A failed, cancelled, or skipped required transition is failed proof.
8. The commit SHA and GitHub Actions run are source and proof identities.
9. The artifact from that run is the delivery output.
10. Facts remain independent backend evidence unless a concrete product change
    explicitly makes one part of release acceptance.
11. Installation is document-driven through INSTALL.md.

## Deterministic PR proof

A pull request to a grok/rust-v* line runs Cargo on GitHub's pull-request
context. PR proof does not build distribution binaries or run real-provider
Live.

The Cargo proof includes:

- Rust formatting;
- Clippy for the affected product owners and tests;
- Provider/API contracts;
- App Server protocol schema and precomputed export consistency;
- Provider-bound App Server tests;
- Grok Core and whole-number argument tests;
- Guardian/memory/history/image-generation owner tests;
- Go Live harness unit tests.

Generated or precomputed outputs are not considered closed merely because their
source fixtures changed. Their stock-owned consistency tests must pass.

## Merge provenance

On a push to grok/rust-v*, the Release provenance job runs before target
builds.

It uses GitHub's own repository state to verify:

1. the pushed SHA is associated with a merged PR;
2. that PR targets the current version line;
3. the PR head has a successful pull_request run of .github/workflows/grok.yml;
4. that run contains a completed successful Cargo job.

The Linux and macOS build jobs depend on this gate. Do not replace this with a
manual proof ledger, commit-message convention, or custom stored state.

Repository branch protection or rulesets may provide an additional preventive
control when available. They are not the proof authority: this workflow must
still reject an unproven push.

## Artifact and Live proof

After Release provenance succeeds, a push runs:

~~~text
x86_64-unknown-linux-musl
    -> complete Grok distribution artifact
    -> Grok Live on grok-bin from that artifact

aarch64-apple-darwin
    -> complete Grok distribution artifact
~~~

Current shipped targets are:

- x86_64-unknown-linux-musl
- aarch64-apple-darwin

Other targets stay out until they have users.

workflow_dispatch can run Live against an existing Linux distribution artifact
selected by binary_run_id. It is diagnostic proof only; it does not create a
new delivery artifact or supply missing PR/merge provenance.

Live runs go test -v directly. The Go harness owns failure diagnostics such as
NOT_PROVEN, stage names, and redacted wire evidence; the workflow does not
parse or reinterpret test results.

## Triage

| RED where | Read | Owner | Next |
|---|---|---|---|
| PR Cargo | failing step and native/consistency test | owning seam | fix the seam, derived artifacts, and owning proof |
| Release provenance | associated PR, PR head workflow, Cargo job | release transition | use a proven PR path; do not retry builds to bypass it |
| target build | compiler/staging output | source or build-grok action | fix and prove on a new PR/push |
| Grok Live | NOT_PROVEN stage and redacted wire evidence | capability, egress, ingress, or harness | fix the actual owner; do not add a blind retry |
| TestFact* | recorded vs observed class | corresponding whitelist row | update evidence and product behavior only when user-visible |

A green target build with red provenance or red Live is not completed Grok
proof.

## Delivery

After a SUCCESSFUL push proof, use the artifact from that exact run and target.
The artifact already contains the complete distribution.

## Proof authorities

~~~text
cargo fmt/clippy/test
    -> deterministic PR gate

codex-app-server-protocol consistency tests
    -> generated/precomputed closure

Release provenance
    -> proven PR-to-version-line transition

build-grok action
    -> complete per-target distribution artifact

go test -run '^TestGrok'
    -> Live on the Linux distribution artifact

GitHub Actions run
    -> proof orchestration and immutable run context

INSTALL.md
    -> human/agent installation procedure
~~~

Workflow mechanics live in .github/workflows/grok.yml.
Target staging lives in .github/actions/build-grok/action.yml.
