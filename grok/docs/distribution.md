# Runtime evidence and optional distribution

Development policy lives in [carry-forward.md](./carry-forward.md). This document
owns only backend-evidence and package boundaries.

Carrying an established ability to validate or package the product is different
from executing it. The selected capability must be complete; its existence must
not be reported as a successful run. Actual downloadable artifacts are optional.

## Subjects and authority

Exact stock is a trusted upstream input, not a downstream stock-CI result. C0
accepts the development mechanism; neither establishes the correctness of a
modified Grok runtime or current backend behavior. A green canonical prefix
covers the downstream deterministic obligations defined by the development
policy; it is not a claim that all upstream CI, Live, or package builds ran.

Facts observe a backend. Product Live tests an explicitly selected binary/runtime
composition against the backend. Artifact smoke tests the package humans receive.
Keep those subjects and conclusions separate, even when one workflow hosts them.

Normal delivery uses an exact accepted version-line SHA. Diagnostic builds of
other revisions are allowed when clearly identified. A successful build or Live
run never promotes that source into an accepted baseline, and a missing artifact
does not revoke source acceptance. A demonstrated product defect remains a real
finding regardless of which layer discovered it.

## Workflow responsibilities

The version line's `grok.yml` keeps PR and canonical-push deterministic proof in
the same-revision local `grok-proof.yml`. C0 may bootstrap that path thinly;
subsequent owners add delta, affected-stock, and composition proof. Backend calls
and release builds do not enter that required native contract. New package or
invocation mechanisms still require their own deterministic tests in it, while
coverage of earlier applicable downstream obligations remains intact.

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
availability. Its deterministic unit/harness tests belong to the shared native
proof as introduced by their owners. Sensitive backend credentials are supplied
only to approved backend jobs, never exposed to untrusted PR execution. Missing
credentials and opt-in skips are not passing backend evidence.

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
build actions to new stock. Test new staging/orchestration behavior and affected
composition; do not compensate for missing semantic tests with a release build.
A stock source/fixture/lock adaptation genuinely needed for delivery is a named,
tested downstream increment, not an implicit C0 repair or an excuse to weaken
package assertions. Preparation of required build inputs is distinct from stock
behavioral re-certification.

## Report and triage

Record exact subjects, native command/result, artifact identities, and redacted
failure evidence. Label historical and not-rerun observations honestly.

- Native failure: owning source/test or deterministic CI seam.
- Facts discrepancy: recorded backend observation versus the actual response.
- Live failure: product, backend, environment, or harness, determined by evidence.
- Build/staging/smoke failure: package unavailable or unusable for that target.

Do not repair a test by weakening its contract solely to make the run green.
Backend refresh is parallel unless the product contract explicitly requires it.
Keep package execution separate from source initialization acceptance; concrete
defects affecting relied-on contracts still require triage.

A pending workflow result is not evidence. Bind any later conclusion to the
exact run/attempt/subject and its authoritative settled status. Follow current
user instructions for execution/monitoring, without a mandatory bot dependency.
Do not add blind reruns, proof ledgers, publisher state machines, or post-merge
historical PR searches. Practical artifact retention may remove a package without
altering its accepted source history. Policy publication is not implementation,
execution evidence, or delivery.
