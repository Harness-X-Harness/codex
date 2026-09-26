---
name: grok-release-evolution
description: Bootstrap and route Grok stable-tag carry-forward, release proof, historical diagnosis, and optional grok/main experiments through the canonical evolution doctrine. Do not use for ordinary stock Codex work.
---

# Grok Release Evolution

This skill is the bootstrap and routing entrypoint. It is not a second process
authority.

## Bootstrap

1. Always read the current doctrine from
   grok/main:grok/docs/carry-forward.md before release-evolution work.
2. Do not use the carry worktree as the bootstrap source. A carry branch starts
   from exact stock and may intentionally lack the current process overlay.
3. Resolve the lifecycle state defined by the doctrine before changing code.
4. Resolve the latest SUCCESSFUL grok/rust-v* checkpoint and the exact target
   stock tag/SHA. Do not use an unproven candidate as the baseline.
5. Treat current process/proof decisions on grok/main as mandatory inputs even
   when they postdate the latest stable release.

A release-local copy of this skill or carry-forward.md is historical process
evidence only.

## Reconstruction phase

For carry-forward work:

1. Read the latest successful release's branch-local product authorities needed
   to understand the carried semantics.
2. Read the exact target stock seams and their native tests/generators.
3. Classify candidate semantics according to carry-forward.md.
4. Reconstruct the smallest current semantic at the target seam.
5. For every changed semantic, enumerate owner, invariant, source seam, derived
   representations, and owning proof.
6. Run stock-owned generators and consistency tests before calling the
   candidate RECONSTRUCTED.

Do not mechanically replay old commits, reconstruct historical implementation
shapes, or treat a clean patch application as semantic proof.

## Release handoff

RELEASE_HANDOFF is mandatory for a complete release operation.

At handoff:

1. Read the target version line's branch-local grok/docs/release.md.
2. Verify that the branch-local proof mechanism satisfies the current doctrine.
3. Carry any required proof-mechanism update before starting release proof.
4. From that point, release.md owns PR checks, merge provenance, build targets,
   artifact identity, Live proof, and SUCCESSFUL.

Do not restate release.md rules from memory.

A direct version-line push, a local test run, or a green artifact/Live run does
not substitute for a missing deterministic PR transition.

If the user requested carry, merge, or release end-to-end, continue to the
requested lifecycle state. Do not stop after reconstruction merely because the
code appears ready. Stop only on failed proof, missing authorization, or an
unavailable external capability, and report the exact state reached.

## grok/main work

For process maintenance or experiments on grok/main, follow the main
rebase/prune/rebuild rules in carry-forward.md. Keep the process overlay small
and current. Do not make grok/main a required product-promotion intermediate.

## Historical diagnosis

Read older release lines or Git history only when current doctrine, the latest
successful release, target stock, and directly relevant evidence cannot resolve
a concrete semantic question. Stop once the transition is understood.

## Authorization

Review findings are not write authorization. Do not merge, publish, prune,
rebase, force-reset, or mutate another authoritative branch unless the user or
invoking task authorized that action.

After mutation, verify the authoritative ref and workflow result required for
the requested lifecycle transition. Do not invent ledgers or parsers to replace
GitHub's own state.
