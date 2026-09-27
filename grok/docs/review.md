# Grok product review

This document is the canonical current authority for reviewing the Grok
downstream product and its release evolution.

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

Current release-evolution policy:

- `grok/main:grok/docs/carry-forward.md`

Current product baseline:

- the latest `grok/rust-v*` that reached `SUCCESSFUL` under its branch-local
  `grok/docs/release.md` contract.

A newer release candidate does not replace that authority until it is
`SUCCESSFUL`.

Release-specific product and proof truth stays on that release line. For
forward-looking `grok/main` work, current stock authority is
`openai/codex main`. For a release candidate, stock authority is its exact
target tag/SHA.

## Review scope

Review incrementally.

Start with Grok-related code, docs, workflows, PRs/issues, and proof state that
changed since the previous review. Expand only to the current authorities,
runtime owners, generators, tests, Facts, or Live evidence directly needed to
resolve those changes.

Do not enter older release history for completeness. Use it only when current
evidence cannot resolve a concrete semantic question.

## Review lenses

### Context / legacy / mechanism size

Apply the referenced `context-reduce` and `prune-legacy` methods.

Look for duplicate authorities, stale process snapshots, dead compatibility
paths, and unnecessary scripts, ledgers, parsers, publishers, validators, or
synchronization layers.

For release infrastructure, specifically ask:

- does this mechanism protect an independent invariant?
- is native owner testing or GitHub state already the authority?
- does a helper have stable semantics or reuse beyond one workflow?
- can a script or state layer be deleted without weakening proof?

Flag custom workflow-state models or decision frameworks that merely mirror a
small number of GitHub API predicates.

Also review `grok/main` as a cheap rebased workspace: keep its process overlay
small, preserve it during pruning, and do not accumulate finished experiments.

Finding prefix: `CR-` or `PL-`.

### Stock ownership

For a release candidate, compare affected downstream responsibilities with the
exact target stock tag. For `grok/main`, compare with current
`openai/codex main`.

Report a downstream mechanism when stock now owns the responsibility and Grok
can delete or reduce it without changing the product contract.

Finding prefix: `SO-`.

### Carry closure

For every retained or intentionally changed semantic, verify:

```text
changed owner
-> source-of-truth seam
-> that owner's derived outputs
-> that owner's native proof
```

Do not require a global generated-file sweep. Follow only the owners changed by
the candidate.

Report a candidate as incomplete when an owned derived representation is stale
or a necessary owner-level consistency proof is missing.

Finding prefix: `CC-`.

### Semantic contract / proof

Keep proof classes distinct:

- deterministic implementation proof;
- backend observation;
- exact-artifact Live composition;
- workflow orchestration.

Do not expand backend acceptance, HTTP success, a green PR, or a workflow state
into a product claim it does not prove.

Prefer user-visible semantic invariants over implementation details.

Finding prefix: `CP-`.

### Release authority

Use the simplified product states:

```text
RECONSTRUCTED
PR_PROVEN
SUCCESSFUL
```

Verify:

- `RECONSTRUCTED` includes the changed owners' derived-output closure;
- after reconstruction, the candidate actually hands off to branch-local
  `release.md`;
- `PR_PROVEN` comes from the proof required by that `release.md`;
- only the exact version-line head that completes the release contract becomes
  `SUCCESSFUL`.

Merge and provenance are release transitions owned by `release.md`, not
separate product states.

If a direct or otherwise unproven push can become `SUCCESSFUL`, report the
release mechanism.

Finding prefix: `RP-`.

### Release delta

When a new stable target exists, review only:

```text
latest successful Grok release
+ explicit product decisions
+ relevant validated grok/main experiments
+ exact target stock tag
```

Classify affected semantics as stock-owned, retain downstream, intentional
product change, obsolete, or unclear/history needed. Only the last class
justifies deeper historical diagnosis.

Finding prefix: `RD-`.

## Output

Report only findings backed by direct evidence. For each finding include the
owner, invariant, evidence, smallest correction, correctness boundary, and
risk.

For carry/release review, report the current product state and the first
unsatisfied transition. If there is no meaningful finding, report `none`.

Do not manufacture work to make the review non-empty.

## Mutation boundary

Review is read-only by default.

Do not modify code, docs, PRs, Issues, workflows, tags, releases, or branches
without explicit authorization from the invoking task or user. Findings are not
write authorization.
