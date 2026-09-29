# Grok product review

This document is the canonical current authority for reviewing Grok downstream
product evolution and its supporting distribution machinery.

The scheduled task uses this document as review policy. Automation-specific
mutation permission belongs to the task, not to this document.

## Methods

Use these methods directly; do not restate or fork them here:

- context-reduce:
  https://github.com/ronhuafeng/skills-release/tree/main/catalog/engineering/context-reduce
- prune-legacy:
  https://github.com/ronhuafeng/skills-release/tree/main/catalog/engineering/prune-legacy

Use `prune-legacy` only for review/identification unless an explicit apply or
prune request authorizes mutation.

## Authorities

Current product-evolution policy:

- `grok/main:grok/docs/carry-forward.md`

Current binary-distribution policy:

- `grok/main:grok/docs/distribution.md`

Current accepted product state:

- the newest accepted `grok/rust-v*` source state for its stock generation.

Current continuation baseline:

- that accepted source directly; or
- a verified semantics-preserving baseline rewrite derived from it.

A build, artifact, release page, workflow run, or download channel does not
make source canonical and does not gate continuation to the next stock version.

Release-specific architecture, product docs, Facts, Stories, and implementation
truth stay on the corresponding version line. For forward-looking `grok/main`
work, current stock authority is `openai/codex main`. For a carry candidate,
stock authority is its exact target tag/SHA.

## Review scope

Review incrementally.

Start with Grok-related code, docs, workflows, PRs/issues, accepted baseline
state, and distribution surfaces that changed since the previous review. Expand
only to the current authorities, runtime owners, generators, tests, Facts, or
Live evidence directly needed to resolve those changes.

Do not enter older version-line history for completeness. Use it only when
current evidence cannot resolve a concrete semantic question.

## Review lenses

### Context / legacy / mechanism size

Apply the referenced `context-reduce` and `prune-legacy` methods.

Look for duplicate authorities, stale process snapshots, dead compatibility
paths, and unnecessary scripts, ledgers, parsers, publishers, validators, or
synchronization layers.

For process infrastructure, specifically ask:

- does this mechanism protect an independent invariant?
- is native owner testing or repository state already the authority?
- does a helper have stable semantics or reuse beyond one workflow?
- can a script or state layer be deleted without weakening semantic validation
  or distribution usability?

Flag workflow-state models or decision frameworks that merely mirror a small
number of GitHub API predicates.

Also review `grok/main` as a cheap rebased workspace: keep its process overlay
small, preserve it during pruning, and do not accumulate finished experiments.

Finding prefix: `CR-` or `PL-`.

### Stock ownership

For a carry candidate, compare affected downstream responsibilities with the
exact target stock tag. For `grok/main`, compare with current
`openai/codex main`.

Report a downstream mechanism when stock now owns the responsibility and Grok
can delete or reduce it without changing the product contract.

Finding prefix: `SO-`.

### Baseline resolution

Verify that continuation begins from the newest accepted product semantics, not
from whichever SHA most recently produced an artifact.

If a semantics-preserving baseline rewrite is used, verify:

~~~text
accepted source SHA
-> complete diff classification
-> preserved product semantics
-> changed-owner closure where needed
-> explicit separation from intentional product changes
~~~

A rewrite may improve history or remove representation noise without being
distributed. That is not a defect.

Report a baseline problem when:

- the rewrite cannot be traced to an accepted source;
- a difference is unexplained;
- behavior changed under the label of cleanup;
- owner-derived outputs became stale;
- a distribution result is being used as semantic authority.

Finding prefix: `BR-`.

### Carry closure

For every retained or intentionally changed semantic, verify:

~~~text
changed owner
-> source-of-truth seam
-> that owner's derived outputs
-> that owner's native proof
~~~

Do not require a global generated-file sweep. Follow only the owners changed by
the candidate.

Report a candidate as incomplete when an owned derived representation is stale
or a necessary owner-level consistency proof is missing.

Finding prefix: `CC-`.

### Semantic contract / validation

Keep evidence classes distinct:

- deterministic implementation proof;
- backend observation;
- semantic Live evidence;
- distribution smoke evidence;
- workflow orchestration.

Do not expand backend acceptance, HTTP success, a green artifact build, or a
workflow state into a product claim it does not prove.

Prefer user-visible semantic invariants over implementation details.

`RECONSTRUCTED` means candidate semantics and changed-owner closure are complete
on target stock. It does not mean binaries have been produced.

Version-line admission should follow the repository's required semantic checks,
review, and branch protections. Once accepted, the source may seed future carry
regardless of artifact availability.

Finding prefix: `CP-`.

### Distribution independence

Distribution exists only to make binaries convenient for humans to obtain.

Review distribution separately from semantic authority.

Verify:

- each artifact identifies the source SHA it was built from;
- the intended binaries and bundled assets are present;
- optional artifact-level smoke tests actually consume the packaged artifact;
- a failed build is reported as artifact unavailability, not product invalidity;
- artifact success is not used to promote or select a semantic baseline;
- semantic Live checks are not accidentally reclassified as distribution proof,
  or vice versa.

Do not require every accepted source to have downloadable artifacts unless
there is an actual human distribution requirement.

Finding prefix: `DI-`.

### Evolution delta

When a new stable stock target exists, review only:

~~~text
resolved semantic baseline
+ explicit product decisions
+ relevant validated grok/main experiments
+ exact target stock tag
~~~

Classify affected semantics as:

- stock-owned;
- retain downstream;
- intentional product change;
- obsolete;
- unclear/history needed.

Only the last class justifies deeper historical diagnosis.

Finding prefix: `ED-`.

## Output

Report only findings backed by direct evidence. For each finding include the
owner, invariant, evidence, smallest correction, correctness boundary, and
risk.

For carry review, report:

- resolved semantic baseline;
- target stock authority;
- whether owner closure is complete;
- whether the candidate is `RECONSTRUCTED`;
- the first unsatisfied semantic validation or admission requirement, if any.

For distribution review, report artifact availability and integrity separately.
Do not translate distribution status into product authority.

If there is no meaningful finding, report `none`.

Do not manufacture work to make the review non-empty.

## Mutation boundary

Review is read-only by default.

Do not modify code, docs, PRs, Issues, workflows, tags, artifacts, releases, or
branches without explicit authorization from the invoking task or user.
Findings are not write authorization.
