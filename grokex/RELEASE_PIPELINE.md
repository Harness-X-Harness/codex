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
scenario is `always` required or required when its `seam_paths` changed, and
which paths belong to that seam. `live_smoke.py` reads deadlines from it and
binds `contract_sha256` into every evidence file; evidence produced under a
different contract cannot be composed into a release.

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
`live_smoke.py`, or the contract file requires every scenario again.

## Validator carrier

A `grokex-live` run may execute from a later commit than the candidate source
when the product-to-carrier diff touches only validation paths (`release.py`,
`live_smoke.py`, their tests, the contract file, this document, and
`.github/workflows/grokex-*.yml`). `release.py verify-carrier` enforces the
allowlist; the archive under test still comes from the product SHA.

## Observation mode

`grokex-observe` (dispatch, or nightly schedule when present on the default
branch) runs every scenario against the newest published release with
`--mode observation`. Deadlines and semantic failures are recorded, never
gating, and `release.py summarize-observations` aggregates `turn_durations_seconds`
into p50/p95/max per scenario against the contract deadline. Observation
evidence is rejected by `build-live-evidence`.
