---
name: grok-release-evolution
description: Route Grok stable-tag carry-forward, release proof, historical diagnosis, and optional grok/main experiments through the repository's canonical evolution doctrine. Do not use for ordinary stock Codex work.
---

# Grok Release Evolution

This skill is an agent entrypoint, not a second process authority.

## Start here

1. Read the current evolution doctrine from
   `grok/main:grok/docs/carry-forward.md`.
2. Follow that document for branch roles, main rebase/prune maintenance,
   release baselines, semantic selection, history use, feedback, and backports.
3. For release work, read the selected release line's branch-local product
   authorities as needed:
   - `grok/docs/architecture.md` for runtime/product invariants;
   - `grok/docs/release.md` for proof and delivery;
   - `grok/docs/request-whitelist.md` when Provider, Responses, tools,
     history, search, SSE, or Images projection is involved;
   - the actual runtime seams and native tests you touch.
4. Use the correct stock authority:
   - release work: the exact target stock tag/SHA;
   - `grok/main` work: current `openai/codex main`, following the
     rebase/prune rules in `carry-forward.md`.
5. Read older release lines or Git history only when the current doctrine and
   directly relevant evidence are insufficient.

A release-local copy of this skill or `grok/docs/carry-forward.md`, if one
exists on an older release, is historical process evidence. It is not the
current evolution authority.

## Execute narrowly

Use the shortest path that preserves the current product contract. Do not
mechanically replay old commits, reconstruct historical implementation shapes,
or make `grok/main` a required intermediate.

Keep product/runtime truth on the release line that owns it. Keep current
release-evolution policy in the canonical doctrine on `grok/main`.

## Authorization

Review findings are not write authorization. Do not merge, publish, prune,
rebase or force-reset a branch, mutate another authoritative branch, or
repeatedly monitor CI unless the user has authorized that action.

After a mutation, verify the authoritative ref or workflow result needed to
confirm the requested effect. Do not invent ledgers, parsers, or compatibility
state to prove GitHub's own state.
