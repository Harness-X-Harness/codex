# Grokex release pipeline

Three workflows compose one publication. Each stage binds the exact source SHA
and the exact Linux archive digest, so stages can be re-run independently
without rebuilding or re-proving what already holds.

| Stage | Workflow | Trigger | Produces |
|-------|----------|---------|----------|
| 1 | `grokex-candidate` | push to `release/**`, or dispatch | `grokex-linux-candidate` artifact (14 days): the Linux archive plus `CANDIDATE.json` (source SHA, archive digest, whether the deterministic gates ran or were reused) |
| 2 | `grokex-live` | dispatch with `candidate_run_id` (or `release_tag`) and an optional `scenarios` subset | one `grokex-live-evidence-<scenario>` artifact per scenario (30 days) |
| 3 | `grokex-release` | dispatch with `candidate_run_id` and `live_run_ids` | the immutable `grokex-vX.Y.Z` tag and release assets |

## Identity

`grokex/release-source.json` is the single place that names the upstream tag,
upstream commit, and version. Workflows read it through
`python3 grokex/release.py identity`, so bumping a release changes one file.

## Deterministic gates run once per SHA

`grokex-candidate` skips `grokex-checks` when a check run named
`Require every deterministic gate` is already GREEN for the same SHA (for
example from the pull request that fast-forwarded the branch). A superseded push
cancels the in-flight candidate for that branch.

## Live scenarios and the contract file

`grokex/live_contracts.json` is the executable contract behind the Live
Stories: for each scenario it names the Story, the Turn deadline, whether the
scenario is `always` required or required when its `seam_paths` changed, which
paths belong to that seam, and which `oracle` proves it. The oracle reads the
deadline from the contract and binds `contract_sha256` into every evidence
file; evidence produced under a different contract cannot be composed into a
release.

Every scenario names the `canonical_session` oracle, `grokex/validator` (Go):
it drives a real task through the exact generated protocol client (`codexsdk`
at the same upstream commit as `release-source.json`) in a fresh `CODEX_HOME`,
lets the app-server end normally, then proves the Story from the persisted
session rollouts under `sessions/`, the saved artifacts, and the reply the
app-server delivered. The validator answers app-server requests fail-closed:
approvals are declined, the one client-owned dynamic tool of the continuation
scenario (`grokex_live_probe`) is answered with its fixed marker, anything else
fails the run as a harness failure. Notification kinds, tool-call names, item
types, server-request counts, and timings are written as diagnostics only; a
deadline expiry records `last_proven_stage` and the persisted Turn state so the
post-mortem names what the model was doing. Raw rollout lines never leave the
validator; evidence carries labels, booleans, digests, and counts.

`grokex/validator/internal/rollout/testdata` holds real rollouts recorded by
the stock 0.151 recorder under a Grok profile against a mock provider (one
paginated app-server image thread, one legacy exec Ultra thread with its
full-history child, one paginated thread with a dynamic-tool round trip,
encrypted reasoning items, and a history Turn). They pin the rollout shapes the oracle depends on: the
`task_started`/`task_complete` event names, `item_completed` TurnItems with
`Extension`/`image_gen.generation` and `AgentMessage`, and the child file that
starts with its own `session_meta` (`parent_thread_id`, `forked_from_id`)
followed by the parent's inherited head.

Re-run only what failed: dispatch `grokex-live` with the same
`candidate_run_id` and `scenarios: image-generation-history-edit`. Every run
writes evidence for the scenarios it executed; `grokex-release` composes the
completed evidence from all listed `live_run_ids` (later ids take precedence
per scenario) into one `LIVE_EVIDENCE.json` and records `validation_runs`.

Required set: `grokex-release` diffs the candidate source against the source
of the newest published release (or `inherit_from`) and requires every scenario
whose seam changed, plus `basic-exact-reply`. Scenarios whose seams did not
change may be inherited from that published release; the manifest lists them
under `inherited_scenarios` with the release tag, source SHA, archive digest,
and validation run they were proven on. Pass `inherit_from: none` to require
every scenario on the exact artifact.

Any change to `codex-rs/Cargo.lock`, the build action, `release.py`,
`grokex/validator/`, or the contract file requires every scenario again.

Test-only paths never require a Live re-proof because they cannot change the
shipped binary: files under a `tests/` directory, `*_tests.rs`, and `tests.rs`
(see `test_only_paths` in the contract). A product module that merely declares
a test module still counts as a seam change.

## Seam pins and Grok test placement

Grok-specific tests never live inside stock test files, so an upstream bump
rebases them without conflicts:

- `codex-rs/core/src/tools/router/flat_projection_tests.rs` and
  `seam_pins_tests.rs` (declared from the Grok `flat_projection` module)
- `codex-rs/core/tests/suite/grok_*.rs`
- `codex-rs/codex-api/src/provider_grok_tests.rs`
- `codex-rs/model-provider/src/grok_provider_tests.rs`

`seam_pins_tests.rs` and `provider_grok_tests.rs` are executable assumptions
about stock shapes the graft depends on: every `ToolSpec` variant is classified
as local or provider-hosted, every stock `CollabAgentTool` states whether Grok
restores it as a plaintext call, the `spawn_agent` argument contract stays
`{message, task_name}`, and the top-level `ResponsesApiRequest` fields forwarded
to the Grok gateway are an explicit allowlist. When an upstream bump breaks a
pin, the graft is reviewed instead of failing first in a Live scenario.

## Validator carrier

A `grokex-live` run may execute from a later commit than the candidate source
when the product-to-carrier diff touches only validation paths (`release.py`,
`seam_series.py`, their tests, `grokex/validator/`, the contract and seam map
files, this document, and `.github/workflows/grokex-*.yml`).
`release.py verify-carrier` enforces the allowlist; the archive under test
still comes from the product SHA.

`grokex-release` honors the same chain: the candidate's source may be an
ancestor of the release-branch head, and the Live validator may sit between
them, as long as every step is validation-only. The product SHA (the
candidate's source) is what the remaining five targets are built from, what
the archives and `RELEASE.json` name, and what the tag points at; `release.py`
and the evidence composer run from the head, and the shipped `grokex/` files
are packaged from a separate checkout of the product SHA.

## Seam series for upstream bumps

`grokex/seam_series.json` maps every path the graft touches to one of ten
seams. `python3 grokex/seam_series.py plan` fails when a changed path has no
owner; `export --out DIR` writes the net upstream-to-head difference as ten
`git am`-compatible patches, one per seam; `verify --out DIR` applies them onto
the upstream tree and requires the exact release tree hash. `grokex-checks`
runs all three, so the series is always current.

To port to a new upstream tag: export the series from the current release
branch, `git am` it onto the new tag one seam at a time, resolve conflicts
inside that seam only, and let the seam pins and stock controls point at the
shapes that moved.

## Observation mode

`grokex-observe` (dispatch, or nightly schedule when present on the default
branch) runs every scenario against the newest published release with
`--mode observation`. Deadlines and semantic failures are recorded, never
gating, and `release.py summarize-observations` aggregates `turn_durations_seconds`
into p50/p95/max per scenario against the contract deadline. Observation
evidence is rejected by `build-live-evidence`.
