# Runtime evidence and optional distribution

Development policy lives in [carry-forward.md](./carry-forward.md). This document
owns only backend-evidence and package boundaries.

Carrying an established ability to validate or package the product is different
from executing it. The selected capability must be complete; its existence must
not be reported as a successful run. Actual downloadable artifacts are optional.

## Subjects and authority

Facts observe a backend. Product Live tests an explicitly selected binary/runtime
composition against the backend. Artifact smoke tests the package humans receive.
Keep those subjects and conclusions separate, even when one workflow hosts them.

Normal delivery uses an exact accepted version-line SHA. Diagnostic builds of
other revisions are allowed when clearly identified. A successful build or Live
run never promotes that source into an accepted baseline, and a missing artifact
does not revoke source acceptance. A demonstrated product defect remains a real
finding regardless of which layer discovered it.

## Workflow responsibilities

The version line's `grok.yml` always keeps PR and canonical-push deterministic
proof in the shared native `grok-proof.yml`. Backend calls and release builds do
not enter that required native contract.

Once selected release capability is integrated, version-line pushes may also
build the supported packages and run Live on the same-run Linux artifact. Each
requested target builds once; downstream Live consumes that build's package, not
another run or a second build. Failed build means dependent Live is not proved.
Sibling target failures remain visible. Do not label an artifact produced before
product acceptance as a released product merely because a push created it.

Manual artifact-backed Live is diagnostic: require an explicit existing run or
artifact, identify its source/target and the harness revision, and reject missing,
expired, ambiguous, or inappropriate inputs. Do not pick "latest successful" as
an undocumented selector. No diagnostic run substitutes for native PR proof or
post-squash exact-head proof.

Use `grok-facts.yml` for opt-in backend observations, independently of package
availability. Its deterministic unit/harness tests already belong to the shared
native proof. Sensitive backend credentials are supplied only to approved backend
jobs, never exposed to untrusted PR execution. Missing credentials and opt-in
skips are not passing backend evidence.

Use the actual repository's supported events and workflow-dispatch availability.
Do not promise an invocation merely because YAML contains `workflow_dispatch`;
verify registration, permissions, required inputs, and referenced artifacts.

## Complete human-use package

Keep the supported target set driven by actual users. A package includes the
runtime binary and helper binaries it needs, launch/install assets, and the
profile/catalog owned by the product. It identifies repository, exact source SHA,
and target. Run identity and checksums aid retrieval/integrity, not source
acceptance.

Installation instructions cover executable bits, entrypoints, bundled assets,
and a dedicated product Home/configuration. Never silently overwrite or migrate
unrelated user state. Profile/catalog behavior is implemented by its semantic
owner, not invented during packaging.

Preserve the package's observable layout and supported scenarios when adapting
build actions to new stock. Test new staging/orchestration behavior where useful;
do not compensate for missing semantic tests with a release build.

## Report and triage

Record exact subjects, native command/result, artifact identities, and redacted
failure evidence. Label historical and not-rerun observations honestly.

- Native failure: owning source/test or deterministic CI seam.
- Facts discrepancy: recorded backend observation versus the actual response.
- Live failure: product, backend, environment, or harness, determined by evidence.
- Build/staging/smoke failure: package unavailable or unusable for that target.

Do not repair a test by weakening its contract solely to make the run green.
Backend refresh is parallel unless the product contract explicitly requires it.
Keep package execution separate from source initialization acceptance.

A pending workflow result is not evidence. Bind any later conclusion to the
exact run/attempt/subject and its authoritative settled status. Do not add blind
reruns, proof ledgers, publisher state machines, or post-merge historical PR
searches. Practical artifact retention may remove a package without altering its
accepted source history.
