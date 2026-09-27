---
name: grok-release-evolution
description: Bootstrap and route Grok stable-tag carry-forward, release proof, historical diagnosis, and optional grok/main experiments through the canonical evolution doctrine. Do not use for ordinary stock Codex work.
---

# Grok Release Evolution

This skill is the bootstrap and routing entrypoint. It is not a second process
authority.

## Bootstrap

1. Always read the current doctrine from
   `grok/main:grok/docs/carry-forward.md` before release-evolution work.
2. Do not use the carry worktree as the bootstrap source. A carry branch starts
   from exact stock and may intentionally lack the current process overlay.
3. Resolve the latest `SUCCESSFUL grok/rust-v*` checkpoint and the exact target
   stock tag/SHA. Do not use an unproven candidate as the baseline.
4. Treat the current doctrine itself as process authority. Do not invent a
   separate list of process/proof changes to carry as product state.

A release-local copy of this skill or `carry-forward.md` is historical process
evidence only.

## Reconstruction

For carry-forward work:

1. Read only the latest successful release authorities needed to understand the
   selected semantics.
2. Read the exact target stock seams and their native tests/generators.
3. Classify candidate semantics according to `carry-forward.md`.
4. Reconstruct the smallest current semantic at the target owner.
5. For each changed owner, close its source, owned derived outputs, and native
   consistency/regression proof.

Do not mechanically replay old commits, reconstruct historical implementation
shapes, or treat clean patch application as semantic proof.

Call the candidate `RECONSTRUCTED` only when those owner closures are complete.

## Release handoff

For a complete release operation, hand off immediately after
`RECONSTRUCTED`:

1. Read the target version line's branch-local `grok/docs/release.md`.
2. If that release contract is insufficient, repair it before proof.
3. From then on, follow `release.md` as the sole authority for PR proof,
   merge/provenance, builds, artifacts, Live, `PR_PROVEN`, and `SUCCESSFUL`.

Do not restate or infer release mechanics from memory or from
`carry-forward.md`.

If the user requested carry-and-release end to end, continue until
`SUCCESSFUL` unless required proof fails, authorization is missing, or an
external capability is unavailable.

## grok/main work

For process maintenance or experiments on `grok/main`, follow the
rebase/prune/rebuild rules in `carry-forward.md`. Keep the process overlay
small and current. Do not make `grok/main` a product-promotion intermediate.

## Historical diagnosis

Read older release lines or Git history only when the current doctrine, latest
successful release, target stock, and directly relevant evidence cannot resolve
a concrete semantic question. Stop once the transition is understood.

## Mechanism discipline

Prefer native owner tests, the branch-local release workflow, and minimal
native GitHub protection. Do not create custom ledgers, workflow-state models,
or helper frameworks whose only purpose is to mirror GitHub state for one
workflow.

## Authorization

Review findings are not write authorization. Do not merge, publish, prune,
rebase, force-reset, or mutate another authoritative branch unless the user or
invoking task authorized that action.

After mutation, verify the authoritative ref or workflow result required for
the requested state transition. Do not invent compatibility state to prove
GitHub's own state.
