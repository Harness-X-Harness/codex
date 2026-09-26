# Grok release evolution

This document is the canonical current authority for Grok product evolution.
The repo-local skill is only an agent entrypoint and must not duplicate these
rules.

Release branches own their exact product state and proof. A release-local copy
of this document or the evolution skill, if present on an older release, is a
historical process snapshot rather than current policy.

The stable release sequence is the product spine. `grok/main` is a rebased
stock-main workspace for current process guidance and optional short-lived
experiments. It is not the product authority.

## Authorities

```text
latest successfully proven grok/rust-v*
    = default semantic baseline for the next release

exact target stock rust-vX.Y.Z
    = target harness and stock-behavior authority

explicit product decisions since the previous release
    = intentional semantic changes

relevant validated grok/main experiments
    = optional candidate inputs

older successful grok/rust-v*
    = historical evidence when a carry needs diagnosis

carry/grok-rust-vX.Y.Z
    = temporary reconstruction / review workspace
```

Do not use an unproven release candidate as the successful baseline.

## Stable release spine

The normal product evolution path is:

```text
latest successful grok/rust-vN
        +
explicit product changes
        +
selected useful experiments, if any
        +
exact stock rust-vN+1
        ↓
semantic classification and reconstruction
        ↓
carry/grok-rust-vN+1
        ↓
review
        ↓
grok/rust-vN+1
        ↓
exact build + Live proof
        ↓
next successful product checkpoint
```

The goal is not to reproduce the previous release implementation. The goal is
to preserve or intentionally change its product semantics on the new stock
architecture.

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

## Historical releases

The latest successful release is a normal baseline for the next release.

Older successful releases are valuable when a carry meets resistance because
they record known-good Grok compositions on exact stock tags. They are normally
frozen; later improvements are not backported by default.

Use older history to understand transitions, not to replay implementations:

- why a downstream mechanism existed;
- whether it was a stable product contract or stock-version workaround;
- where the owning stock seam moved;
- which product semantic survived the change.

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
upstream main. Do not depend on a local Git remote name such as `origin`; the
authority is `openai/codex main`.

Prefer rebase to merging stock main into `grok/main`. This workspace does not
need a merge history that records routine upstream movement.

The process overlay is not an experiment. It is the minimum valid final state
of `grok/main` and survives routine pruning.

Use these terms consistently:

```text
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
```

When there are no active experiments, keep the downstream process overlay as
one commit where practical. When an experiment remains useful, rebase that
intent onto the current stock-main seams. When an experiment is stale or its
lesson has been captured elsewhere, prune the experiment and leave the process
overlay intact.

Rebase, prune, or rebuild `grok/main` does not change any release line and is not:

- carry-forward;
- backport;
- promotion from or to a `grok/rust-v*` branch.

The branch does not need to continuously reproduce the released product. Its
purposes are:

- keep current process guidance close to current stock architecture;
- explore major upcoming stock seam changes when that knowledge is useful;
- try product ideas before selecting them for a stable release;
- test release lessons against current stock main when useful.

Its experiment contents do not automatically belong in the next release.
Pruning experiments does not destroy product knowledge; the stable release
spine and Git history retain the successful checkpoints. The process overlay
remains because it owns current evolution and review policy.

## Carry workspace

A matching `carry/grok-rust-vX.Y.Z` branch is temporary review work. It starts
from the exact target stock tag/version line and reconstructs the selected
semantic set.

Do not make it another long-lived product authority.

Construct commits by current semantic owner. Fold historical probes, fixes,
schema refreshes, and cleanup when they now describe one settled behavior.
Provider-neutral fixes remain provider-neutral.

## Proof and release authority

A version line is not successful merely because its PR is green.

```text
Facts
    = backend observations

PR
    = deterministic native proof

version-line push
    = exact target distributions

Live
    = real-provider composition on the exact Linux artifact
```

After the required build and Live proof succeeds,
`grok/rust-vX.Y.Z` is the release authority for that exact stock tag and a
new checkpoint in the stable release spine.

## Feedback

Release work may produce useful lessons. Record them even when they do not
change the release.

A lesson can later be re-expressed on `grok/main`, but this is optional and
does not determine continuity of the next release. The next release starts from
the latest successful release checkpoint.

When feeding a lesson to main:

```text
preserve the lesson
not the patch
```

Re-evaluate it against current stock main and use the current seam.

## Backports

Do not routinely modify historical successful release lines. Backport only
when a concrete support, correctness, or security requirement is explicitly
requested.
