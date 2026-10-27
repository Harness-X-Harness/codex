# Identified package-to-Live invocation

`fetch_artifact.py` acquires one explicit Linux package from GitHub Actions.
It inherits an existing authenticated official `gh` environment; it does not
log in, configure credentials or execute the package. Python 3.11 or later and
an already authorized Actions artifact read context are required.

```sh
python3 grok/package/fetch_artifact.py \
  --artifact-id EXPLICIT_ARTIFACT_ID \
  --harness-sha "$(git rev-parse HEAD)" \
  --output /existing-parent/new-selection
```

Selection starts with one immutable GitHub artifact ID. Its metadata and
controlled name identify the repository, source, target, producer run and original
producer attempt. The original attempt must contain exactly one successful,
completed Linux build. The current workflow attempt and unrelated jobs do not
choose or invalidate that package: Live can consume it while macOS is running,
after a sibling failure, or when a partial rerun reuses the original build.

The selector verifies the trusted `grok.yml` version-line push, or the explicit
same-repository PR diagnostic contract below. Missing, expired or misidentified
artifacts and incomplete producer-job inventories are rejected. No other
artifact, run or latest-success fallback is selected. Inventories above 100 jobs
fail closed.

The command uses bounded, read-only `gh api` calls for the exact artifact,
its producer run, the original attempt's jobs, and that artifact's ZIP. PR
selection additionally reads the linked PR. Each call has a 180-second limit,
metadata is capped at 16 MiB, and the package is capped at 2 GiB. Raw API
responses, signed URLs and authentication errors are not retained in published
evidence. There is no automatic retry.

Acquisition failures include one fixed, allowlisted stage code in brackets:

- `input`: explicit arguments or an already-existing output were rejected.
- `workspace`: private temporary workspace creation or cleanup failed.
- `artifact_request`, `run_request`, `jobs_request`: that bounded metadata
  request failed, including command startup, timeout, response-size or I/O errors.
- `artifact_metadata`, `run_metadata`, `jobs_metadata`: the response could not
  be read as metadata. Producer identity is checked before using it.
- `selection`: the existing run/producer/artifact identity or inventory checks,
  or the selected package byte budget, rejected the metadata.
- `archive_request`: the bounded selected ZIP request failed.
- `archive_verification`: archive or package verification/extraction failed.
- `publication`: writing or publishing the verified selection failed.
- `unknown`: an otherwise handled failure had no recognized stage code.

Selection failures also carry one fixed `selection=` reason: `run_state`,
`jobs_inventory`, `identity`, `producer` or `artifact`.
These distinguish a rejected PR state from an inconsistent inventory without
publishing metadata. Unclassified selection errors retain only the stage code.

Failed `gh` requests may include a fixed allowlisted `gh=` category and `http=`
status recognized from bounded stderr. Unknown, ambiguous or oversized stderr
produces no extra summary. Raw stderr, URLs, tokens and API bodies are never
copied to output. These categories describe what `gh` reported, not a proven
network or service root cause.

Codes identify the failed boundary, not the underlying cause. For example,
`artifact_request` alone does not establish a network error, rate limit or
availability race. Exception text, API bodies, credentials and signed URLs
never become stage codes or handled acquisition error output.
Before granting executable modes, extraction verifies the Actions ZIP size and
SHA-256, the exact archive name, all required stock-layout members, source and
target metadata, and every packaged file's size/digest. Links, traversal,
duplicate or extra members and incomplete helpers are rejected. The new output
contains `package/`, `selection.json` and `verified.json`; an existing output is
never replaced. Failed private extraction cannot become a runnable output.

From the exact harness checkout, build `grok-live` and select an owned scenario:

```sh
(cd grok && go build -o /existing-parent/grok-live ./cmd/grok-live)
/existing-parent/grok-live --help
```

The caller supplies the absolute verified runtime path, its `runtime_sha256`
from `verified.json`, the selected source/target, the actual harness SHA and an
execution-environment label. The command checks the executable digest and runs
one selected owner-developed scenario once. Real execution additionally requires
the existing explicit `GROK_LIVE=1` context and approved `GROK_API_KEY`; neither
successful retrieval nor compilation constitutes invocation authorization.
See `../cmd/grok-live/README.md` for outcomes and the retained scenario surface.

## Automatic Actions consumption

On every trusted version-line push, the full Live matrix still depends directly
on the successful Linux build and scenario selection. The trusted execution step
sets `GROK_LIVE=1`; this automatic path makes real API calls and incurs their
costs. Ordinary PR events do not run Live or receive the backend secret.

The build publishes an artifact ID. Live passes that ID to the same fetcher used
by diagnostics and additionally requires `--expected-source-sha "$GITHUB_SHA"`.
The producer's original attempt comes from the selected artifact, never the
consumer's `GITHUB_RUN_ATTEMPT`. Retrying Live does not require rebuilding the
package or waiting for macOS. Producer success, trusted source and complete
artifact verification remain required before the backend secret is used.

The Go-owned full scenario selection runs as separate jobs;
fail-fast is disabled so one failure cannot hide other outcomes. Parallelism is
bounded to three. Each invocation has one five-minute whole-scenario deadline,
no scenario retry, and a typed result containing only owner-approved evidence.
Source, target, archive/runtime hashes, selection identity and each result are
retained as `grok-live-SCENARIO-SHA-RUN-ATTEMPT` for 30 days. Missing configuration,
artifact failure, timeout, cancellation, backend failure and skipped jobs remain
distinct from a successful observed result.

The build jobs and Live matrix do not enter required native `Cargo` admission.
A packaging or product defect still needs diagnosis; independence does not hide
a failed sibling or turn it green. Formal evidence is the actual GitHub Actions
run/attempt/test subject. A local CLI run is only development/diagnostic evidence.

## Explicit diagnostic Actions path

The separate `grok-live.yml` workflow selects one explicit Go-owned selection
(a diagnostic scenario or the smoke/full suite) and a required
`binary_artifact_id`. It has no native `Cargo` job, and cannot substitute for required
PR/push admission. On an explicit version-line invocation it uses the original
`GROK_LIVE=1` context and the already approved backend secret; no repository
variable is necessary for that explicit request.

Once the selected harness ref contains the workflow, the operator can submit:

```sh
gh workflow run 355014891 --repo Harness-X-Harness/codex \
  --ref grok/rust-v0.164.0 -f binary_artifact_id=EXPLICIT_ARTIFACT_ID \
  -f suite=diagnostic -f scenario=basic-primary
```

The ID is the existing `.github/workflows/grok-live.yml` registration. Registry
state and a declared trigger are not proof of availability. Verify the actual
submission, matching run/attempt, harness ref, selected artifact and scenario
result.

For a version-line diagnostic, the selected package source need not equal the
harness SHA. Both identities and the producer run's observed current status remain in
evidence. No
fallback source is substituted, and a successful invocation does not certify
source eligibility or other unexecuted scenarios. Actual retained backend results
belong to #331; release/source acceptance and registration are separate claims.

For a PR diagnostic, choose the current same-repository PR branch and pass
`--pr-diagnostic` to the fetcher (the workflow derives this from the harness ref).
The current open PR head must match the harness and build-run head, its
version-line base and synthetic merge source must match the package, and PR CI
must have succeeded. This separate PR admission policy is not replaced by
artifact availability. Forks and stale heads remain rejected before backend use.

`selection.json` records producer `run_id`/`run_attempt`, artifact identity and
observed run status. `workflow.json` records the consumer's run/attempt. A passed
single diagnostic does not change a failed or cancelled original run, and is not
proof of other scenarios or required native CI.
