# Grok delivery

This document owns the branch-local Grok proof and delivery contract. Product
semantics live in architecture.md.

Once carry-forward reaches RECONSTRUCTED and hands the candidate to this
version line, this document is the sole authority for the remaining proof and
delivery transitions. The handoff is an authority transition, not a product
state.

## Northstar

~~~text
PR to grok/rust-v*
    = deterministic Cargo proof, including generated/precomputed closure

push grok/rust-v*
    = GitHub-admitted version-line head
    -> complete target distributions
    -> Linux Live on the same-run Linux artifact

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

The branch-local durable product states are exactly:

~~~text
RECONSTRUCTED
    -> required PR proof
PR_PROVEN
    -> merge to the version line
    -> exact-head release proof
SUCCESSFUL
~~~

RECONSTRUCTED is established by carry-forward after candidate semantics and
owned derived outputs are closed on the target stock. Handoff from that state
makes this branch-local document authoritative; handoff itself is not another
state.

PR_PROVEN requires the required `pull_request` Grok workflow to complete
successfully for the candidate in the target `grok/rust-v*` PR context. The
public `Cargo` aggregate must be successful. Local tests do not substitute for
this state.

Merge is a transition, not a durable state. A PR_PROVEN candidate may enter the
version line only through the permitted merge path. The resulting authoritative
head must then complete the push release contract for that exact SHA.

SUCCESSFUL requires that exact version-line head to complete the release
workflow successfully, including all required shipped target artifacts and
Grok Live consuming the exact Linux artifact required by this contract.
Native branch admission is the merge transition. Target builds and Live are
proof conditions within the push release transition; they are not additional
product states.

A direct or otherwise unproven push cannot become SUCCESSFUL. Every new
authoritative `grok/rust-v*` head requires a new release proof for that exact
SHA. Proof from an earlier head is never inherited, including when the new head
changes only documentation or other non-runtime files.

## Rules

1. PR review owns deterministic proof: formatting, lint, native regression
   tests, harness unit tests, and stock-owned generated/precomputed consistency
   tests for the surfaces this product changes.
2. When a PR changes App Server protocol, protocol source, schema, or generated
   SDK surfaces, Cargo must run the App Server protocol schema/precomputed
   export consistency suite so checked-in schemas cannot diverge from embedded
   precomputed exports. Unrelated proof-infrastructure or documentation changes
   do not retroactively reopen inherited generated surfaces.
3. The active GitHub Ruleset owns admission to `grok/rust-v*`: changes enter
   through a PR, the GitHub Actions `Cargo` check is required and strict, and
   deletion, force-push, and bypass are blocked.
4. After GitHub admits a new version-line head, the push workflow trusts that
   platform transition and proves release composition for the admitted state.
   It does not reconstruct branch admission by searching historical PR runs or
   comparing PR head refs/SHAs.
5. Each shipped target builds exactly once for the push run.
6. Linux Live depends on the Linux build and consumes the Linux distribution
   artifact from that same workflow run.
7. A failed, cancelled, or skipped required build or Live transition is failed
   proof; the head is not SUCCESSFUL.
8. The commit SHA is source/artifact identity, not a reconstructed admission
   predicate.
9. The GitHub Actions run is the release proof unit and its artifacts are the
   delivery outputs.
10. Facts remain independent backend evidence unless a concrete product change
    explicitly makes one part of release acceptance.
11. Installation is document-driven through INSTALL.md.

## PR_PROVEN proof

A pull request to a grok/rust-v* line runs Cargo on GitHub's pull-request
context. When the required Grok pull-request workflow completes successfully
with its public `Cargo` aggregate successful, the candidate is PR_PROVEN.
This proof does not build distribution binaries or run real-provider Live.

The public `Cargo` status is a stable aggregate gate: every PR proof-domain job
it owns must succeed. Internal proof jobs may evolve without changing the
required status name.

The Cargo proof includes:

- Rust formatting;
- Clippy for the affected product owners and tests;
- Provider/API contracts;
- App Server protocol schema and precomputed export consistency when the PR
  touches that owner/source/generated closure;
- Provider-bound App Server tests;
- Grok Core and whole-number argument tests;
- Guardian/memory/history/image-generation owner tests;
- Go Live harness unit tests;
- structured exact-match editing: engine semantics, runtime/lifecycle behavior, stale-snapshot rejection, VerifiedContents failure semantics, and approval-hook identity;
- stock `apply_patch` compatibility, including nested Code Mode and Linux sandbox/link regressions.

Generated or precomputed outputs are not considered closed merely because their
source fixtures changed. When a PR touches that closure, its stock-owned
consistency tests must run and pass.

## Branch admission and release composition

The required pull-request Grok workflow is the authority for PR_PROVEN. The
active `Grok version lines` GitHub Ruleset is the authority for whether that
PR_PROVEN change may enter `grok/rust-v*`.

The Ruleset requires PR-based changes and the GitHub Actions `Cargo` context
with strict/up-to-date enforcement. It blocks deletion and non-fast-forward
updates and has no bypass. The release workflow trusts those native GitHub
control-plane decisions instead of querying historical PR workflow runs after
merge.

Every admitted version-line head starts a push workflow. That run owns release
composition:

~~~text
push event
    -> Linux distribution build
       -> Linux artifact
       -> Grok Live consumes that artifact in the same run
    -> macOS distribution build
~~~

The workflow DAG and current-run artifact namespace bind these operations.
Commit SHA remains useful for artifact names and observability, but PR head
SHA/ref equality is not a release-authorization predicate.

## Artifact and Live proof

For every admitted version-line push, the release workflow runs:

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
new delivery artifact or substitute for PR_PROVEN, branch admission, or the
required push release workflow.

Live runs go test -v directly. The Go harness owns failure diagnostics such as
NOT_PROVEN, stage names, and redacted wire evidence; the workflow does not
parse or reinterpret test results.

## Triage

| RED where | Read | Owner | Next |
|---|---|---|---|
| PR Cargo | failing step and native/consistency test | owning seam | fix the seam, derived artifacts, and owning proof |
| branch admission | GitHub Ruleset / required Cargo context | native GitHub policy | repair the PR proof or ruleset; do not add workflow-side provenance reconstruction |
| target build | compiler/staging output | source or build-grok action | fix and prove on a new PR/push |
| Grok Live | NOT_PROVEN stage and redacted wire evidence | capability, egress, ingress, or harness | fix the actual owner; do not add a blind retry |
| TestFact* | recorded vs observed class | corresponding whitelist row | update evidence and product behavior only when user-visible |

A green target build with red Live, a failed sibling target build, or missing
native branch admission is not completed Grok proof.

## Delivery

After a SUCCESSFUL push proof, use the artifact from that exact run and target.
The artifact already contains the complete distribution.

## Proof authorities

~~~text
cargo fmt/clippy/test
    -> deterministic PR proof

codex-app-server-protocol consistency tests
    -> generated/precomputed closure

required pull_request Grok workflow / Cargo
    -> PR_PROVEN

Grok version lines GitHub Ruleset
    -> version-line branch admission

push Grok workflow DAG
    -> exact-head release composition

build-grok action
    -> complete per-target distribution artifact

go test -run '^TestGrok'
    -> Live on the same-run Linux distribution artifact

rust-ci-full / Structured edit remote proof
    -> Docker-backed remote executor semantics for structured_edit; independent of the PR release gate

GitHub Actions run
    -> proof orchestration and immutable run context

INSTALL.md
    -> human/agent installation procedure
~~~

Workflow mechanics live in .github/workflows/grok.yml.
Target staging lives in .github/actions/build-grok/action.yml.
