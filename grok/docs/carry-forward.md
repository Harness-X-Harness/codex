# Grok release evolution

This document is the canonical current authority for Grok product evolution.
The repo-local skill is only the bootstrap and routing entrypoint. It must not
fork these rules.

Release branches own their exact product state and branch-local release proof.
A release-local copy of this document or the evolution skill, if one exists on
an older release, is historical process evidence rather than current policy.

The stable release sequence is the product spine. `grok/main` is a rebased
stock-main workspace for current process guidance and optional short-lived
experiments. It is not the product authority.

## Authorities

```text
grok/main:grok/docs/carry-forward.md
    = current evolution policy

latest successful grok/rust-v*
    = default semantic baseline for the next release

exact target stock rust-vX.Y.Z
    = target harness and stock-behavior authority

explicit product decisions since the previous release
    = intentional semantic changes

relevant validated grok/main experiments
    = optional candidate inputs

older successful grok/rust-v*
    = historical evidence when diagnosis is needed

carry/grok-rust-vX.Y.Z
    = temporary reconstruction and review workspace
```

Do not use an unproven release candidate as the successful baseline.

The carry workspace is the work object, not the process bootstrap source.
Start release evolution from the `grok/main` entrypoint so the current doctrine
is read even when the exact stock tag and carry branch intentionally do not
contain the process overlay.

## Stable release spine

The normal evolution path is:

```text
latest successful Grok release
        +
explicit product decisions
        +
selected useful experiments, if any
        +
exact target stock
        ↓
semantic classification and reconstruction
        ↓
owner + derived-output closure
        ↓
RECONSTRUCTED
        ↓
handoff to branch-local release.md
        ↓
required PR proof
        ↓
PR_PROVEN
        ↓
merge to exact version line
        ↓
release.md build + exact-artifact Live proof
        ↓
SUCCESSFUL
```

The goal is not to reproduce the previous implementation. Preserve or
intentionally change product semantics on the new stock architecture.

For every candidate behavior ask:

```text
Does target stock now own it?
    yes -> use stock and drop the downstream mechanism

Does the new release still require it?
    yes -> reconstruct the smallest semantic at the current stock seam

Was the behavior intentionally changed since the last release?
    yes -> implement the new product decision

Is it only a grok/main experiment?
    yes -> omit unless explicitly accepted

Is it an obsolete workaround or historical intermediate state?
    yes -> drop

Is it provider-neutral correctness behavior?
    -> check target stock first; retain only when still needed
```

No-path-conflict is not semantic proof. Nearby upstream churn is not evidence
that Grok needs a patch.

## Product states

Keep only the states that change product authority.

```text
RECONSTRUCTED
    candidate semantics and owned derived outputs are closed on target stock

PR_PROVEN
    branch-local release.md required deterministic PR proof succeeded

SUCCESSFUL
    exact version-line head satisfied the complete branch-local release.md contract
```

Baseline resolution, release handoff, and merge are transitions, not additional
long-lived states.

Only `SUCCESSFUL` becomes the default semantic baseline for the next release.
A build, Live run, direct push, local test, or partially green workflow cannot
substitute for a missing state transition required by `release.md`.

## Semantic closure

For every retained or intentionally changed semantic, close only the owner that
changed:

```text
changed semantic owner
    -> source-of-truth seam
    -> that owner's checked-in or runtime-consumed derived outputs
    -> that owner's native consistency/regression proof
```

Use the target stock tree to discover ownership. Do not keep a global generated
file checklist.

When the owner has a stock generator, run it instead of editing its outputs
independently. Include the resulting owned outputs and run the stock consistency
tests. `RECONSTRUCTED` is not reached while an owned derived representation is
stale.

Do not search the repository for unrelated caches, locks, schemas, SDKs, or
serialized forms merely because they are generated. Closure follows the changed
owner.

If the target owner lacks a generator or consistency test that is necessary for
correctness, add the smallest owner-level proof. Do not create a parallel
downstream generation framework.

## Carry workspace

A matching `carry/grok-rust-vX.Y.Z` branch is temporary review work. It starts
from the exact target stock tag/version line and reconstructs the selected
semantic set.

Do not make it another long-lived product authority.

Construct commits by current semantic owner. Fold historical probes, fixes,
schema refreshes, and cleanup when they now describe one settled behavior.
Provider-neutral fixes remain provider-neutral.

Before release handoff, verify each changed owner is closed and then call the
candidate `RECONSTRUCTED`.

## Release handoff

Once the candidate is `RECONSTRUCTED`, read the target version line's
branch-local `grok/docs/release.md` unconditionally.

From that point, `release.md` is the sole authority for:

- required PR proof and the definition of `PR_PROVEN`;
- the permitted merge/provenance path;
- shipped targets and artifact identity;
- exact-artifact Live proof;
- the definition of `SUCCESSFUL`.

Do not restate those mechanics here.

If the target line's release contract is too weak to prove the candidate
correctly, repair that branch-local release contract before proof. Then continue
according to `release.md`; do not infer release success from this document.

Facts remain backend observations. Whether a Fact gates a release belongs to
that release line's release contract.

## Mechanism boundary

Keep release infrastructure smaller than the product it proves.

Prefer, in order:

1. native owner tests for product semantics;
2. the branch-local GitHub Actions workflow for release orchestration;
3. minimal native GitHub branch protection/rulesets as preventive guardrails.

Do not add a second proof authority merely to test or reinterpret GitHub's own
state. Avoid proof ledgers, persistent synchronization state, publisher state
machines, custom workflow-state models, and helper frameworks whose only job is
to mirror a few GitHub API predicates for one workflow.

Extract decision logic into code only when it has independent stable semantics
or meaningful reuse beyond making the workflow itself testable.

A minimal native protection rule for `grok/rust-v*` may require PR-based
changes, the relevant deterministic check, and protection from force-push or
deletion. It is a preventive guardrail, not the release proof authority.
Do not add approval-count, merge-queue, deployment, signing, or bypass
complexity without a concrete operational need.

## Historical releases

The latest `SUCCESSFUL` release is the normal baseline for the next release.

Older successful releases are useful only when the latest baseline, target
stock, and current doctrine do not explain a semantic cleanly. They are
normally frozen.

Use older history to understand why a semantic existed or where ownership
moved. Do not replay historical implementation sequences.

Inspect the nearest useful history first and stop once the semantic is
understood.

## grok/main

`grok/main` is deliberately cheap to rebase and safe to prune.

Its normal shape is:

```text
current practical openai/codex main
+
one current Grok process overlay
+
optional short-lived experiment commits
```

Normal maintenance is to periodically rebase `grok/main` onto current stock
upstream main. The authority is `openai/codex main`, not a local remote name.

Prefer rebase to merging stock main into `grok/main`.

The process overlay is not an experiment. It is the minimum valid final state
of `grok/main` and survives routine pruning.

Use these terms consistently:

```text
rebase main
    = move the workspace to newer practical stock main while preserving the
      process overlay and useful experiments

prune experiments
    = remove finished, stale, or superseded experiment commits while
      preserving the process overlay

rebuild main
    = reconstruct the single current process-overlay commit on stock main

hard reset to stock
    = temporary mechanical step during rebuild only; pure stock is not the
      normal final state of grok/main
```

When there are no active experiments, keep the process overlay as one commit
where practical. Rebase useful experiment intent to current seams; prune stale
experiments.

Rebase, prune, or rebuild `grok/main` does not change any release line and is
not carry-forward, backport, or product promotion.

## Feedback

Release work may produce useful lessons. A lesson can later be re-expressed on
`grok/main`, but this is optional and does not determine continuity of the
next release.

```text
preserve the lesson
not the patch
```

Re-evaluate it against current stock main and use the current seam.

## Backports

Do not routinely modify historical successful release lines. Backport only
when a concrete support, correctness, security, or explicitly requested
release-mechanism requirement exists.
