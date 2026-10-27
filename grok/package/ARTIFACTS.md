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

The selector authenticates the actual source of a trusted same-repository
`grok.yml` producer, independently of the consumer harness. Push and PR builds
use the same package contract; closing or merging a producer PR does not
invalidate its original successful build. PR checkout identity uses the existing
subject proof, retained for the same 30 days as packages. Previously expired
subject evidence cannot be recovered or replaced by a guess. A complete
inventory proving its absence or expiry makes that producer unavailable to
discovery; an explicit request for that package still fails before execution.
Conflicting identity, invalid digest, unsuccessful proof producer or incomplete
API evidence remain errors, not unavailable candidates. Missing, expired or misidentified
artifacts and incomplete producer-job inventories are rejected. No other
artifact, run or latest-success fallback is selected. Inventories above 100 jobs
fail closed.

The command uses bounded, read-only `gh api` calls for the exact artifact,
its producer run, the original attempt's jobs, and that artifact's ZIP. PR
selection independently reads the current harness PR and its current CI, not
the binary producer's PR as a substitute for harness authorization. Each call has a 180-second limit,
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
- `harness_ci_request`, `harness_ci_metadata`: the independently selected
  current PR CI request or its admission evidence failed.
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

On every trusted version-line push, the full Live matrix depends on either the
successful new Linux build or current controlled proof of the reused package,
and on scenario selection. The trusted execution step
sets `GROK_LIVE=1`; this automatic path makes real API calls and incurs their
costs. Ordinary PR events do not run Live or receive the backend secret.

The scheduler resolves the current version line's latest successful Linux
available, content-compatible producer once. `build_selection.py` compares that source to the complete current
harness subject using the shared coarse classifier in `select_artifact.py`: known consumer-only areas do
not rebuild; unknown paths and binary/build inputs do. Renames and deletions are
included. Unavailable packages and packages with different binary inputs are
not candidates. If none match, a new build is required; a failed required build
does not fall back to a package with different inputs.

The chosen or newly built artifact ID is fixed for all Live scenarios. Live uses
the same fetcher as diagnostics with `--expected-line "$LINE"`, which verifies
the candidate line scope and the complete binary-input diff. It retains the original
package source and the current harness SHA separately.
The producer's original attempt comes from the selected artifact, never the
consumer's `GITHUB_RUN_ATTEMPT`. Retrying Live does not require rebuilding the
package or waiting for macOS. macOS has its own latest successful producer and
independent build decision; Linux availability does not stand in for a Mac package.
Skipping a Mac build retains the original successful producer evidence and
checks its available artifact metadata; it does not download, revalidate archive
bytes or execute current Mac fixtures. Current reused-package acceptance below
applies to the Linux package used by Live. Producer success, trusted source and complete
artifact verification remain required before the backend secret is used.

The Go-owned full scenario selection runs as separate jobs;
fail-fast is disabled so one failure cannot hide other outcomes. Parallelism is
bounded to three. Each invocation has one five-minute whole-scenario deadline,
no scenario retry, and a typed result containing only owner-approved evidence.
Source, target, archive/runtime hashes, selection identity and each result are
retained as `grok-live-SCENARIO-SHA-RUN-ATTEMPT` for 30 days. Missing configuration,
artifact failure, timeout, cancellation, backend failure and skipped jobs remain
distinct from a successful observed result.

The required `Cargo` aggregate combines native proof with successful subject
selection and the applicable Linux build or reuse-proof path. A legitimate
not-needed build is distinct from a failed selection or failed required build.
macOS and actual Live results remain independently visible; they are not
silently converted into native proof. Formal evidence is the actual GitHub Actions
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

For a PR diagnostic, choose its current same-repository branch. The workflow
passes `--harness-ref` from that ref; this independently resolves exactly one
open PR whose current head matches the harness, whose base is a version line,
and whose current head/base has successful `grok.yml` CI. The existing
`grok-subject.json` artifact proves the actual tested head/base/merge: its archive
digest and original successful `Execution subject` producer are verified,
without binding it to the current rerun attempt or introducing another schema.
The current merge object is separately fetched and verified; a workflow run's
head SHA is not misrepresented as its checkout merge SHA.
The binary producer does not grant harness admission.

A trusted push or PR package in the candidate line scope then goes through
the same artifact/content checks. Binary eligibility compares its real source
to the current PR merge subject, not the PR head: the shared full binary-input
diff must leave build inputs unchanged. The fixed PR merge ref is
fetched read-only and checked against the admitted merge SHA; a moved subject
fails. No credentials are installed. Stale/forked PRs or stale CI remain rejected
before backend use; the original package source is never relabelled.

`selection.json` records producer `run_id`/`run_attempt`, artifact identity and
observed run status. `workflow.json` records the consumer's run/attempt. A passed
single diagnostic does not change a failed or cancelled original run, and is not
proof of other scenarios or required native CI.

## Build applicability and current proof

`grok.yml` dispatches; `grok-build.yml` owns native target/runner/build settings;
the existing `build-grok` action owns compilation and staging. The release
package decision does not suppress native-proof or Go-contract applicability.
`verify_provider.sh` owns the current controlled package acceptance command and
is shared by fresh builds and reuse checks; changing it does not change the
binary, but its current checks still run. This is artifact reuse, not a compiler
cache, a new artifact registry, or reuse of old test results.

An eligible build is an available published package with a verified successful
original producer and unchanged binary inputs. Candidate order is producer workflow run number,
then the artifact's original producer attempt, descending, restricted to the
candidate version-line scope. Push refs and the current producer PR base ref
filter that scope; they do not prove a unique historical branch name. Actual
source comes from the producer evidence, not the PR's current merge/head. Copied job IDs or execution timestamps
from a partial rerun do not invent another package or producer identity. Whole-workflow and Live conclusions do not establish producer success.
Git commits and the complete source-to-subject diff own content compatibility.
Identical trees naturally have an empty diff, including after rebase or squash;
no event-specific content rule or source-ancestry requirement is used.
The planner resolves metadata and the fixed ID without downloading the package.
The consumer downloads and verifies archive/manifest/runtime hashes before
execution. The decision records actual binary/harness subjects.
Existing non-version-line PR routes build normally without line reuse.

Candidate discovery examines the latest 100 `grok.yml` runs, newest first,
and stops at the first trusted, available, content-compatible package. This is
an optimization window, not a proof about all build history. After a successful
read of that window, no match means `no_matching_recent_artifact` and a new
build; an older matching package outside the window can therefore be rebuilt.
Known absent or expired packages are skipped. API, identity, digest and Git
diff errors remain explicit errors rather than being disguised as a cache miss.
