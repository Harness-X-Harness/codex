# Grokex release pipeline

One GitHub Actions run is the evidence that a release is valid. Everything
else in this directory exists to give that run its identities, its archives,
its oracle, and an independent check of what it published.

```
PR ──► grokex-checks        deterministic gate: Rust contract tests and seam pins,
                            model-visible request snapshots, validator tests,
                            seam series, release helper contracts
push ► grokex-build         Linux archive for the product tree (cached by tree)
dispatch ► grokex-release   identities → six archives (cached) → four Live
                            scenarios → assemble → publish once → verify
schedule ► grokex-release   same build and Live jobs in observation mode
```

## Identity

`grokex/release-source.json` is the only place that names the upstream tag,
the upstream commit, and the Grokex version. `release.py identity` derives the
release tag from it and computes the **product tree**: a digest over the git
tree objects of every path that feeds a build (`codex-rs`, `grokex/dist`,
`grokex/release-source.json`, `.github/actions`, `.github/scripts`). Two
commits with the same product tree are the same product. Everything outside
those paths (`grokex/validator`, `release.py`, the contract, the seam tools,
the workflows, this document) is validation or tooling by definition; there is
no allowlist to maintain.

Files that ship inside the archives live in `grokex/dist`.

## Path contract for checks

`grokex/check_scope.py` maps the paths a pull request changes to the gate
groups `grokex-checks` runs: `rust` (formatting, the cargo contract tests, the
library lints; about 25 minutes cold) and `helpers` (the Go validator, the
release helpers, the seam series, the shipped scripts, and an `actionlint` pass
over the Grokex workflows; about one minute). `codex-rs/` and `.github/actions/`
need `rust`; `grokex/` (except Markdown), the Grokex workflow files, and
`LICENSE` need `helpers`; Markdown, `docs/`, and stock workflow files need
nothing. A path no rule names needs every gate, so an unforeseen file can only
make the run slower. The checks workflow itself, and every event that is not a
pull request, need every gate. The `Require every deterministic gate` job
accepts a skipped gate only when the contract said the change did not need it.

## Build once per product tree

`grokex-build` builds one archive per target and uploads it as the artifact
`grokex-build-<product_tree>-<target>` (90 days). Before building it looks for
a non-expired artifact with that name in any earlier run and skips the build
when one exists. A push to `release/**` builds the Linux target so the release
run and the nightly observation start from a warm cache; `grokex-release`
calls the same workflow for all six targets. A commit that changes only
validators, workflows, or documentation keeps the product tree and rebuilds
nothing. The macOS x86_64 target takes about two hours; this is paid once per
product tree.

`PROVENANCE.json` inside each archive names the product tree and the commit
the archive was built from; verification compares the tree, so an archive
built at an earlier commit with the same product tree is the same product.

## Live scenarios

`grokex/live_contracts.json` names each scenario's Story and Turn deadline.
`grokex/validator` (Go, on the exact `codexsdk` app-server client pinned to the
same upstream commit as `release-source.json`) is the only oracle: it drives a
real task through the exact protocol in a fresh `CODEX_HOME`, lets the
app-server end normally, and proves the Story from the persisted session
rollouts under `sessions/`, the saved artifacts, and the reply the app-server
delivered. It answers app-server requests fail-closed: approvals are declined,
the one client-owned dynamic tool of the continuation scenario
(`grokex_live_probe`) is answered with its fixed marker, anything else fails
the run as a harness failure. Notification kinds, tool-call names, item types,
server-request counts, and timings are diagnostics; a deadline expiry records
`last_proven_stage` and the persisted Turn state. Raw rollout lines never leave
the validator.

Every release run executes every scenario against the Linux archive of the
product tree it publishes. There are no scenario subsets, no inheritance from
earlier releases, and no evidence composition across runs; a failed scenario
fails the run and the run is re-dispatched (about fifteen minutes with a warm
build cache). Per-scenario evidence files are uploaded as run artifacts for
diagnosis and summarized into `RELEASE.json`.

`grokex/validator/internal/rollout/testdata` holds real rollouts recorded by
the stock 0.151 recorder under a Grok profile against a mock provider (a
paginated app-server image thread, a legacy exec Ultra thread with its
full-history child, a paginated thread with a dynamic-tool round trip and
encrypted reasoning). They pin the rollout shapes the oracle depends on.

## Publication

`grokex-release` refuses to start when the tag or Release already exists,
rechecks that the branch head is still the run's commit right before
publishing, creates the tag on that commit with the six archives, the dist
files, `RELEASE.json`, and `SHA256SUMS`, then downloads the published Release
and verifies it against the same identities (`release.py verify-assets`).
`dry_run: true` does everything except create the tag and Release; use it to
rehearse after pipeline changes.

`RELEASE.json` records the tag, version, upstream commit, source commit,
product tree, release run id, and the status and Turn durations of every
scenario as proven in that run.

## Seam pins and Grok test placement

Grok-named tests live in Grok-owned test modules, so an upstream bump rebases
them without conflicts. When a Grok test needs a stock test file's private
helpers, the stock file carries only a `#[path = "..."] mod ...;` declaration
and the test body lives in the Grok module:

- `codex-rs/core/src/tools/router/grok_tests.rs`, which declares
  `flat_projection_tests.rs` and `seam_pins_tests.rs` beside it (the flat
  projection itself is the provider-neutral `codex_tools::flat_projection`
  module; the router only holds the routes and the restore hook)
- `codex-rs/core/tests/suite/grok_*.rs`
- `codex-rs/app-server/tests/suite/v2/grok_*.rs` (declared from the stock
  suite file whose helpers they reuse); `grok_provider_binding.rs` is the
  deterministic gate for the Thread Provider Binding Stories (fork, cold
  restart and resume, manual compaction) against a mock Grok gateway
- `codex-rs/codex-api/src/provider_grok_tests.rs` and
  `codex-rs/codex-api/tests/clients/grok_tests.rs`
- `codex-rs/model-provider/src/grok_provider_tests.rs`

Provider-neutral policy tests are different: when a common-file hook makes a
stock code path handle "this Provider has no model policy" (guardian review,
memories, history normalization), the regression test for that hook belongs
next to the stock tests it extends, because it protects stock behavior for
every non-OpenAI Provider rather than a Grok fact.

`seam_pins_tests.rs` and `provider_grok_tests.rs` are executable assumptions
about stock shapes the graft depends on: every `ToolSpec` variant is classified
as local or provider-hosted, every stock `CollabAgentTool` states whether Grok
restores it as a plaintext call, the `spawn_agent` argument contract stays
`{message, task_name}`, the top-level `ResponsesApiRequest` fields forwarded
to the Grok gateway are an explicit allowlist, and the stock OpenAI provider
keeps canonical history unchanged. When an upstream bump breaks a pin, the
graft is reviewed instead of failing first in a Live scenario.

`codex-rs/core/tests/suite/grok_model_visible_request.rs` snapshots the
`/responses` request grok-4.6 receives for each Live Story's first Turn against
a mock gateway: every top-level field, the exact tool inventory with a
description and parameter digest per tool, and the input item kinds. Per-run
identifiers are reduced to their key names. A bump that renames a tool,
collapses a shell type, adds an instruction block, or changes the projected
reasoning effort shows up as a snapshot diff in the PR. Review the `.snap.new`
and accept with `cargo insta accept -p codex-core` only when the visible
change is intended.

## Seam series for upstream bumps

`grokex/seam_series.json` maps every path the graft touches to one seam;
`grokex/seam_series.py` regroups the net difference between the upstream
commit and the release head into one `git am`-compatible patch per seam.
`python3 grokex/seam_series.py plan` fails when a changed path has no owning
seam, `export --out DIR` writes the series, and `verify --out DIR` applies the
patches onto the upstream tree in a scratch index and requires the exact
release tree hash. `grokex-checks` runs all three so the series stays a
faithful, reviewable decomposition of the graft; a new upstream tag is adopted
by applying the series on top of it and updating `release-source.json`.
