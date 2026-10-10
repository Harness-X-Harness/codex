# Identified package-to-Live invocation

`fetch_artifact.py` acquires one explicit Linux package from GitHub Actions.
It inherits an existing authenticated official `gh` environment; it does not
log in, configure credentials or execute the package. Python 3.11 or later and
an already authorized Actions artifact read context are required.

```sh
python3 grok/package/fetch_artifact.py \
  --run-id EXPLICIT_RUN_ID \
  --harness-sha "$(git rev-parse HEAD)" \
  --output /existing-parent/new-selection
```

Diagnostic selection requires a settled version-line push of this repository's
`grok.yml`, its current attempt's successful Linux build, and exactly one
nonexpired source/target/run/attempt artifact. A failed sibling or earlier Live
result remains a failed run; a valid completed Linux producer may still supply a
diagnostic artifact. PR builds, forks, missing/ambiguous/expired artifacts and
incomplete API inventories are rejected. No other run or latest-success fallback
is selected. Inventories above 100 jobs or artifacts fail closed.

The command makes at most four bounded, read-only `gh api` calls: the explicit
run, its exact attempt's jobs, that run's artifacts, and the selected ZIP. Each
call has a 180-second limit, metadata is capped at 16 MiB, and the complete
package is capped at 2 GiB. API responses, signed URLs and authentication errors
are not retained in published evidence. Errors are summarized without raw
external output. There is no automatic retry.

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

## Same-run Actions path

On a trusted version-line push, the Live matrix depends directly on the Linux
build. The workflow uses the same existing `GROK_LIVE` opt-in, exposed by the
execution context's repository variable; absence or any value other than `1`
leaves Live skipped, never accepted. No workflow step creates that variable or
configures a credential. Ordinary PR events do not receive the backend secret.

Each selected scenario binds the current source/harness/run/attempt and the
Linux producer's returned artifact ID. It verifies and consumes that artifact
without rebuilding. The 19 existing scenario selections are separate jobs;
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

The separate `grok-live.yml` workflow selects one retained scenario and a required
`binary_run_id`. It has no native `Cargo` job, and cannot substitute for required
PR/push admission. On an explicit version-line invocation it uses the original
`GROK_LIVE=1` context and the already approved backend secret; no repository
variable is necessary for that explicit request.

Once the selected harness ref contains the workflow, the operator can submit:

```sh
gh workflow run 355014891 --repo Harness-X-Harness/codex \
  --ref grok/rust-v0.158.0 -f binary_run_id=EXPLICIT_RUN_ID -f scenario=basic-primary
```

The ID is the existing `.github/workflows/grok-live.yml` registration. Registry
state and a declared trigger are not proof of availability. Verify the actual
submission, matching run/attempt, harness ref, selected artifact and scenario
result. Historical dispatch failures stay recorded; only an actual new result
can establish current invocation. Stock `main` remains unchanged. Until that
verification succeeds, actual manual invocation remains an open #339 obligation.

The selected package may be diagnostic and its source need not equal the harness
SHA. Both identities and the original run conclusion remain in evidence. No
fallback source is substituted, and a successful invocation does not certify
source eligibility or other unexecuted scenarios. Actual retained backend results
belong to #331; release/source acceptance and registration are separate claims.
