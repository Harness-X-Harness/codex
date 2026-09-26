# Grok product review

This document is the canonical current authority for reviewing the Grok
downstream product and its release evolution.

The scheduled task uses this document as the review policy. Any
automation-specific maintenance permission belongs to the task itself, not to
this document.

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

- the latest `grok/rust-v*` whose required build and Live proof completed
  successfully.

A newer `grok/rust-v*` whose proof is incomplete is a candidate only. It
does not replace the latest successful release authority.

Release-specific product and proof truth stays on that release line, including
its runtime, tests, architecture, delivery contract, Stories, Facts, Live
evidence, catalog, configuration, and request-projection evidence.

For forward-looking `grok/main` work, current stock authority is
`openai/codex main`. For a release candidate, stock authority is that
candidate's exact target tag/SHA. Do not substitute rolling stock main for an
exact-tag release decision.

## Review scope

Review incrementally.

Start with Grok-related code, docs, workflows, PRs/issues, and proof state that
changed since the previous review. Expand only to the current authorities,
runtime seams, tests, Facts, or Live evidence directly needed to understand
those changes.

Do not read older releases or Git history for completeness. Enter older history
only when current evidence cannot resolve a concrete question.

## Review lenses

### Context / legacy

Apply the referenced `context-reduce` and `prune-legacy` methods.

Look for evidence-backed duplicate authorities, stale historical process in
current paths, dead compatibility or fallback paths, legacy schemas/APIs/tests/
docs without a current consumer, and unnecessary scripts, ledgers, parsers,
publishers, validators, or synchronization layers.

Also review `grok/main` as a rebased workspace:

- is its stock base reasonably current relative to `openai/codex main`?
- when no experiment is active, is the downstream overlay still one current
  process commit where practical?
- are extra commits intentional active experiments rather than accumulated
  history?
- has routine upstream movement been merged instead of rebased without a
  concrete reason?
- can a finished or stale experiment be pruned while preserving the process
  overlay?
- does any proposed prune accidentally delete `carry-forward.md`,
  `review.md`, the thin evolution Skill, or the README index?
- if main was hard-reset to stock during maintenance, was the process overlay
  rebuilt before treating the branch as complete?

Finding prefix: `CR-` or `PL-`.

### Stock ownership

For a release candidate, compare affected Grok-specific downstream
responsibilities with that candidate's exact target stock tag.

For forward-looking `grok/main` experiments, compare with current
`openai/codex main`.

Report a downstream mechanism when the applicable stock authority now owns the
required responsibility and the Grok layer can be removed or reduced without
changing the product contract.

Finding prefix: `SO-`.

### Semantic contract / proof

Check that tests, Stories, Facts, Live, and workflow results prove what current
product documentation claims.

Keep proof classes distinct:

- deterministic implementation proof;
- backend observation;
- exact-artifact Live composition;
- workflow orchestration.

Do not expand backend acceptance, HTTP success, a green PR, or a workflow state
into a product claim it does not prove.

Prefer user-visible semantic invariants over accidental implementation details.

Finding prefix: `CP-`.

### Release delta

When a new stable target or candidate release exists, review only:

```text
latest successful Grok release
+ explicit product changes
+ relevant validated grok/main experiments
+ exact target stock tag
```

Classify affected semantics as:

- stock-owned;
- retain downstream;
- intentional product change;
- obsolete;
- unclear / history needed.

Only the last class justifies deeper historical diagnosis.

Finding prefix: `RD-`.

## Output

Report only findings backed by direct evidence. For each finding include:

- current owner;
- invariant it should protect;
- evidence of the problem;
- extra context or legacy burden imposed;
- smallest delete / merge / rewrite action;
- correctness boundary that must remain;
- risk.

If there is no meaningful finding, report `none`.

Do not manufacture work to make the review non-empty.

## Mutation boundary

Review is read-only by default.

Do not modify code, docs, PRs, Issues, workflows, tags, releases, or branches
without explicit authorization from the invoking task or user. Findings are
not write authorization.
