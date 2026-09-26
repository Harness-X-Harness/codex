# Grok release evolution

This document is the canonical current authority for Grok product evolution.
The repo-local skill is only the bootstrap and routing entrypoint. It must not
fork these rules.

Release branches own their exact product state and branch-local release proof.
A release-local copy of this document or the evolution skill, if one exists on
an older release, is historical process evidence rather than current policy.

The stable release sequence is the product spine. grok/main is a rebased
stock-main workspace for current process guidance and optional short-lived
experiments. It is not the product authority.

## Authorities

~~~text
grok/main:grok/docs/carry-forward.md
    = current evolution policy and required state transitions

latest successfully proven grok/rust-v*
    = default product-semantic baseline for the next release

exact target stock rust-vX.Y.Z
    = target harness and stock-behavior authority

explicit product decisions since the previous release
    = intentional semantic changes

current process/proof decisions
    = mandatory release-mechanism changes newer than the last stable release

relevant validated grok/main experiments
    = optional candidate inputs

older successful grok/rust-v*
    = historical evidence when a carry needs diagnosis

carry/grok-rust-vX.Y.Z
    = temporary reconstruction and review workspace
~~~

Do not use an unproven release candidate as the successful baseline.

The carry workspace is the work object. It is not the process bootstrap
source. Start release evolution from the grok/main entrypoint so the current
doctrine is read even when the exact stock tag and carry branch do not contain
the process overlay.

## Stable release spine

The normal product evolution path is:

~~~text
latest successful grok/rust-vN
        +
explicit product changes
        +
current process/proof changes
        +
selected useful experiments, if any
        +
exact stock rust-vN+1
        ↓
semantic classification and reconstruction
        ↓
carry/grok-rust-vN+1
        ↓
semantic + derived-artifact closure
        ↓
release handoff
        ↓
branch-local release.md proof
        ↓
grok/rust-vN+1 successful checkpoint
~~~

The goal is not to reproduce the previous release implementation. The goal is
to preserve or intentionally change its product semantics on the new stock
architecture while also carrying the current proof mechanism required to
establish the next successful checkpoint.

For every candidate behavior ask:

~~~text
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
~~~

No-path-conflict is not semantic proof. Nearby upstream churn is not evidence
that Grok needs a patch.

## Lifecycle state machine

Treat release evolution as explicit state transitions rather than a loose
checklist.

~~~text
BASELINED
    -> RECONSTRUCTED
    -> RELEASE_HANDOFF
    -> DETERMINISTICALLY_PROVEN
    -> MERGED
    -> ARTIFACT_PROVEN
    -> SUCCESSFUL
~~~

BASELINED means the latest successful Grok release, exact target stock tag,
explicit product decisions, and current process/proof decisions are resolved.

RECONSTRUCTED means the selected semantics are expressed at the target stock
seams and semantic closure is complete, including derived artifacts and owning
deterministic tests.

RELEASE_HANDOFF means the target version line's branch-local
grok/docs/release.md has been read and has become the sole authority for the
remaining proof and delivery transitions.

DETERMINISTICALLY_PROVEN means the release.md-required PR proof succeeded for
the candidate. Local tests or a later push build do not substitute for this
state.

MERGED means the proven candidate entered the exact version line through the
release.md-authorized merge path. A direct push is not a substitute for a
proven PR transition.

ARTIFACT_PROVEN means the exact version-line head produced every required
distribution and passed the release.md-defined exact-artifact Live proof.

SUCCESSFUL means the complete release.md contract is satisfied. Only then does
the version line become the default semantic baseline for the next release.

Do not infer or skip states. In particular, a successful build or Live run
cannot retroactively supply missing deterministic PR proof.

## Semantic closure

A carried semantic is not complete merely because the source code compiles or
the user-visible path appears to work.

For every retained or intentionally changed semantic, identify this closure:

~~~text
owner
    = current stock/downstream module that owns the invariant

invariant
    = product behavior that must survive

source seam
    = source-of-truth code/configuration for the behavior

derived representations
    = generated, vendored, serialized, embedded, precomputed, schema, SDK,
      lock, cache, or other checked-in/runtime representations derived from
      that seam

owning proof
    = native consistency/regression tests at the current owner
~~~

Use the target stock tree to discover the closure. Do not maintain a permanent
hard-coded list of generated files in this doctrine because stock ownership
moves.

When a source seam has a stock-owned generator:

1. run that generator rather than editing its outputs independently;
2. include every changed generated or precomputed representation;
3. run the stock-owned consistency tests that compare source, fixtures, and
   embedded/precomputed outputs;
4. inspect the resulting diff for unexpected generated churn.

RECONSTRUCTED is not reached while a derived representation is stale.

If a stock generator or consistency test does not exist, record the gap as a
release finding and add the smallest owner-level proof needed. Do not create a
parallel downstream generator merely to satisfy this procedure.

## Historical releases

The latest successful release is the normal product baseline for the next
release.

Older successful releases are useful when current doctrine, the latest
successful baseline, and the target stock tree do not explain a semantic
cleanly. They are normally frozen; later improvements are not backported by
default.

Use older history to understand transitions, not to replay implementations:

- why a downstream mechanism existed;
- whether it was a stable product contract or a stock-version workaround;
- where the owning stock seam moved;
- which product semantic survived the change.

Inspect the nearest useful history first and stop once the semantic is
understood.

## grok/main

grok/main is deliberately cheap to rebase and safe to prune.

Its normal shape is:

~~~text
current practical openai/codex main
+
one current Grok process overlay
+
optional short-lived experiment commits
~~~

Normal maintenance is to periodically rebase grok/main onto current stock
upstream main. Do not depend on a local Git remote name such as origin; the
authority is openai/codex main.

Prefer rebase to merging stock main into grok/main. This workspace does not
need merge history for routine upstream movement.

The process overlay is not an experiment. It is the minimum valid final state
of grok/main and survives routine pruning.

Use these terms consistently:

~~~text
rebase main
    = move the workspace to a newer practical openai/codex main
      while preserving the current process overlay and useful experiments

prune experiments
    = remove finished, stale, or superseded experiment commits
      while preserving the current process overlay

rebuild main
    = reconstruct the single current process-overlay commit on stock main,
      normally after discarding accumulated experiment history

hard reset to stock
    = a temporary mechanical step during rebuild only;
      pure stock is not the normal final state of grok/main
~~~

When there are no active experiments, keep the downstream process overlay as
one commit where practical. When an experiment remains useful, rebase that
intent onto current stock-main seams. When an experiment is stale or its lesson
has been captured elsewhere, prune it and leave the process overlay intact.

Rebase, prune, or rebuild grok/main does not change any release line and is not
carry-forward, backport, or promotion from or to a grok/rust-v* branch.

The branch does not need to continuously reproduce the released product. Its
purposes are to keep current process guidance close to stock architecture,
explore major upcoming seam changes, try product ideas, and test release
lessons against current stock main.

## Carry workspace

A matching carry/grok-rust-vX.Y.Z branch is temporary review work. It starts
from the exact target stock tag/version line and reconstructs the selected
semantic set.

Do not make it another long-lived product authority.

Construct commits by current semantic owner. Fold historical probes, fixes,
schema refreshes, and cleanup when they now describe one settled behavior.
Provider-neutral fixes remain provider-neutral.

Before release handoff, review the candidate by semantic owner and verify that
each changed source seam has its derived-artifact closure and owning proof.
Generated-file freshness is part of reconstruction, not cleanup after release.

## Release handoff

carry-forward.md owns evolution through RECONSTRUCTED. It does not duplicate
release mechanics.

At the transition to RELEASE_HANDOFF:

1. read the target version line's branch-local grok/docs/release.md;
2. verify that its proof mechanism satisfies the current evolution doctrine;
3. if current process/proof decisions require stronger release mechanics than
   the baseline release contains, carry those mechanics as an explicit process
   change before proof;
4. from this point, release.md is the sole authority for required PR checks,
   merge provenance, shipped targets, artifact identity, Live proof, and the
   definition of SUCCESSFUL.

Do not copy those details back into this document.

A release.md that can declare success after a direct unproven push, or that
does not test owner-level consistency for changed generated/precomputed
surfaces, is weaker than this doctrine and must be corrected before the
candidate can become SUCCESSFUL.

Facts remain backend observations. Whether a Fact gates a particular release is
owned by that release line's release.md and workflow.

When the user requests a complete carry-and-release operation, do not stop at
RECONSTRUCTED, a green local test, or a green push build. Continue through the
release.md state transitions until SUCCESSFUL unless a required proof fails or
an external authorization/capability is unavailable. Report the exact state
where progress stopped.

## Feedback

Release work may produce useful lessons. Record them even when they do not
change the release.

A lesson can later be re-expressed on grok/main, but this is optional and does
not determine continuity of the next release. The next release starts from the
latest SUCCESSFUL release checkpoint.

When feeding a lesson to main:

~~~text
preserve the lesson
not the patch
~~~

Re-evaluate it against current stock main and use the current seam.

## Backports

Do not routinely modify historical successful release lines. Backport only
when a concrete support, correctness, security, or release-mechanism requirement
is explicitly requested.
