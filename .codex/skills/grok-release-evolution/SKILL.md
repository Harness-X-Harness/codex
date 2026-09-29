---
name: grok-release-evolution
description: Bootstrap and route Grok stable-tag carry-forward, semantic baseline resolution, optional binary distribution, historical diagnosis, and grok/main experiments through the canonical evolution doctrine. Do not use for ordinary stock Codex work.
---

# Grok Release Evolution

This skill is the bootstrap and routing entrypoint. It is not a second process
authority.

## Bootstrap

1. Always read the current doctrine from
   `grok/main:grok/docs/carry-forward.md` before product-evolution work.
2. Read `grok/main:grok/docs/distribution.md` only when downloadable artifacts
   are relevant to the request.
3. Do not use the carry worktree as the bootstrap source. A carry branch starts
   from exact stock and may intentionally lack the current process overlay.
4. Resolve the newest accepted `grok/rust-v*` source state and the exact target
   stock tag/SHA.
5. If a verified semantics-preserving baseline rewrite exists, evaluate it under
   the baseline-resolution rules in `carry-forward.md` and use it when it is the
   cleaner continuation representation.
6. Treat current doctrine as process authority. Do not infer semantic authority
   from workflow runs, artifacts, release pages, or download channels.

A release-local copy of this skill or `carry-forward.md` is historical process
evidence only.

## Baseline resolution

Before reconstruction:

1. Name the accepted source SHA.
2. Decide whether to use it directly or use a verified semantics-preserving
   rewrite derived from it.
3. For a rewrite, classify every diff and verify that product behavior is
   unchanged.
4. Treat intentional behavior changes as explicit product decisions, not
   cleanup.
5. Do not require distribution of the rewrite.

A baseline rewrite is disposable continuation history, not another product
trunk.

## Reconstruction

For carry-forward work:

1. Read only the accepted baseline authorities needed to understand the selected
   semantics.
2. Read the exact target stock seams and their native tests/generators.
3. Classify candidate semantics according to `carry-forward.md`.
4. Reconstruct the smallest current semantic at the target owner.
5. For each changed owner, close its source, owned derived outputs, and native
   consistency/regression proof.

Do not mechanically replay old commits, reconstruct historical implementation
shapes, or treat clean patch application as semantic proof.

Call the candidate `RECONSTRUCTED` only when those owner closures are complete.

## Semantic validation and version-line admission

After `RECONSTRUCTED`:

1. Run the required semantic checks for the changed owners.
2. Use the repository's normal review and branch-admission controls.
3. Admit the accepted source to `grok/rust-vX.Y.Z`.
4. Treat that accepted source, or a later verified semantics-preserving baseline
   rewrite derived from it, as eligible input to future carry-forward.

Do not wait for binary distribution before continuing product evolution.

Do not invent product states that merely mirror PR or workflow phases.

## Binary distribution

Binary distribution is optional and has one purpose: make binaries convenient
for humans to download.

When the user asks for downloadable artifacts:

1. Read `grok/main:grok/docs/distribution.md`.
2. Select the exact source SHA to build.
3. Build and package the requested supported targets.
4. Run artifact-level smoke checks only where they protect download usability.
5. Upload or publish the artifacts and report their source SHA.

A failed distribution means the artifact is unavailable. It does not revoke
source acceptance or block the next carry.

Semantic Live evidence and distribution smoke evidence are different roles.
Classify each check explicitly.

If the user requested carry-and-distribute end to end, complete semantic
acceptance first, then perform distribution as a separate derivative operation.

## grok/main work

For process maintenance or experiments on `grok/main`, follow the
rebase/prune/rebuild rules in `carry-forward.md`. Keep the process overlay
small and current. Do not make `grok/main` a product-promotion intermediate.

## Historical diagnosis

Read older version lines or Git history only when the current doctrine, accepted
baseline, target stock, and directly relevant evidence cannot resolve a concrete
semantic question. Stop once the transition is understood.

Historical `release.md` files describe branch-local distribution processes used
at the time. They are not current semantic authority.

## Mechanism discipline

Prefer native owner tests, native repository admission, minimal branch
protection, and simple distribution jobs. Do not create custom ledgers,
workflow-state models, or helper frameworks whose only purpose is to mirror
GitHub state for one workflow.

## Authorization

Review findings are not write authorization. Do not merge, publish artifacts,
prune, rebase, force-reset, or mutate another authoritative branch unless the
user or invoking task authorized that action.

After mutation, verify the exact source ref or external artifact effect that the
requested operation actually owns. Do not use distribution state as semantic
proof.
