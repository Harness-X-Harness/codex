# Grok stock adoption

This guide owns the Grok product-evolution doctrine across the active
integration head and exact-tag release lines. Runtime semantics stay in
[`architecture.md`](./architecture.md), and delivery semantics stay in
[`release.md`](./release.md).

The executable agent workflow is
[`.codex/skills/grok-release-evolution/SKILL.md`](../../.codex/skills/grok-release-evolution/SKILL.md).

## Branch roles

The branches have different authorities:

```text
grok/main
    = integration / experimentation head
    = stock Codex main + current Grok work
    = primary source of candidate downstream semantics
    = not release authority

grok/rust-vX.Y.Z
    = Grok release line on exact stock rust-vX.Y.Z
    = release authority for that exact version after proof
    = successful historical composition after release
    = normally frozen

carry/grok-rust-vX.Y.Z
    = temporary semantic translation and review workspace
    = targets grok/rust-vX.Y.Z
    = no product authority of its own
```

This is not a promotion chain. A successful version line does not replace
`grok/main`, and `grok/main` is not copied wholesale into a release line.

## Product evolution loop

The normal loop is:

```text
stock Codex main
    + ongoing Grok experiments and fixes
    -> grok/main

grok/main candidate semantics
    + exact target stock tag
    -> classify
    -> carry/grok-rust-vX.Y.Z
    -> grok/rust-vX.Y.Z
    -> exact build + Live proof

release lessons
    -> re-evaluate against current stock main
    -> re-express useful semantics on grok/main
```

The flow is bidirectional in knowledge, not in branch ancestry. Carry-forward
preserves current release-worthy semantics on an older exact stock tag.
Feedback preserves lessons discovered during release work on the newer stock
main seam.

## Carry-forward rule

Start with two primary inputs:

1. The current `grok/main` tree, which shows what Grok is currently
   experimenting with and which downstream semantics/fixes exist.
2. The exact target stock tag/SHA, which owns the architecture and stock
   behavior for that release.

Do not assume every `grok/main` behavior belongs in the release.

For every candidate downstream behavior, classify it:

```text
Does target stock now own this behavior?
    yes -> use stock; drop the downstream mechanism

Is the semantic still required by this release?
    yes -> carry the smallest semantic at the target stock seam

Is it a main-only experiment?
    yes -> do not carry

Is it an obsolete workaround or historical intermediate state?
    yes -> drop

Is it provider-neutral correctness behavior?
    -> check target stock first; retain provider-neutrally only when still needed
```

If the classification is unclear, diagnose before coding. Do not create a
compatibility layer merely to preserve an old implementation shape.

A clean path-level application is not proof that the semantic still belongs.
Likewise, stock changing nearby code is not evidence that Grok needs an
adaptation.

## Historical successful releases

Historical `grok/rust-v*` lines are valuable because they record successful
Grok compositions on exact stock tags. They are normally frozen after release;
there is usually no reason to backport later improvements into them.

They are diagnostic evidence, not the default carry-forward source.

Use them when a new carry meets resistance and the current integration head
plus target stock tree do not explain the semantic cleanly. Inspect the nearest
successful release first, then only as much earlier history as needed.

Use history to answer:

- Why did this downstream mechanism exist?
- Was it a stable product contract or a stock-version workaround?
- At which stock transition did its owning seam move?
- Which user-visible semantic survived the implementation changes?

Then implement that surviving semantic at the current target seam. Do not
mechanically replay or cherry-pick the historical implementation.

## Current 0.156.1 fixed points

For the current adoption:

```text
grok/main
    = 82d62fe575707988d23bf629017a26c357714e58
    = current integration / experimentation reference

openai/codex rust-v0.156.1
    = b412ff32c417f855c2b2d1581b77058eed87c84b
    = target stock fixed point

grok/rust-v0.156.1
    = b412ff32c417f855c2b2d1581b77058eed87c84b before carry merges

carry/grok-rust-v0.156.1
    = review work targeting grok/rust-v0.156.1
```

For this adoption, stock `0.156.1` adds GPT-6 Sol/Luna catalog and picker
migration behavior and changes local agent-message-board lifecycle/query
implementation. Those stock changes remain stock. Grok continues to use
`grok/dist/models.json` through stock `model_catalog_json`; catalogs are not
merged and the stock picker is not forked.

## Current product boundaries

Keep identity separate:

```text
WHO   = model_provider / Provider profile
HOW   = WireApi::GrokResponses -> ApiDialect::Grok
WHERE = resolved base_url / routing
```

TrustedTunnel is transparent transport/evidence infrastructure. Do not select
Grok behavior by display name, hostname, model name, or TrustedTunnel host.

The product catalog remains:

```text
grok/dist/models.json
    -> stock model_catalog_json
    -> stock ModelsResponse
```

The shipped request slugs are `grok-4.7` and `grok-4.6`, with `grok-4.7`
as the default. `grok-build` is not a shipped request slug.

The distribution remains document-driven and uses a dedicated `CODEX_HOME`.
Do not add an installer or automatic migration of `~/.codex` or `~/.grok`.

## Commit construction

Build a clean semantic stack for the target stock tag. A commit should
correspond to a current semantic owner and include the native tests that prove
it.

Fold probe/fix/cleanup sequences when they now describe one settled semantic.
Do not recreate an obsolete mechanism merely so a later commit can remove it
again.

Provider-neutral fixes stay provider-neutral. Standalone cleanup that only
made an old intermediate state compile does not need to survive.

## Release proof

Keep these authorities separate:

```text
Facts
    = backend observations

PR
    = deterministic native code proof

version-line push
    = complete exact-SHA distributions

Live
    = real-provider composition proof on the exact Linux artifact
```

A green PR is not a completed release proof.

After an explicitly authorized merge to `grok/rust-vX.Y.Z`, the version line
receives its own push build and Live proof. The proven version line remains the
release authority for that exact stock tag.

## Feedback to grok/main

Release work may reveal improvements that should return to the active
integration head. Examples include a simpler stock seam, a provider-neutral
correctness fix, a better regression test, or a clarified product boundary.

Do not merge the release branch back into `grok/main` by default. Instead:

1. State the lesson independently of its release implementation.
2. Inspect current stock main.
3. Check whether stock main already owns the behavior.
4. Confirm the issue or improvement still applies.
5. Find the current seam.
6. Re-express the semantic on `grok/main` with its owning proof.

The rule is:

```text
preserve the lesson
not the patch
```

A release completion report should include `feedback candidates for grok/main`
so useful lessons are not lost. Use `none` when there are no candidates.

## Normal procedure for a new release line

1. Record the exact target upstream tag and SHA.
2. Inspect current `grok/main` product docs, actual runtime seams, tests,
   Facts, Live, and delivery workflow.
3. Compare the stock base relevant to the current Grok work with the target
   stock fixed point.
4. Classify candidate downstream behaviors instead of replaying `grok/main`.
5. Enter historical diagnosis only when current intent plus target stock is
   insufficient to resolve a semantic.
6. Rebuild the selected semantic stack on the matching `carry/*` branch.
7. Run native deterministic proof at the owning seams.
8. Open the PR against the exact `grok/rust-vX.Y.Z` version line.
9. After explicit merge authorization, let the version-line push build exact
   artifacts and run Live.
10. Record release lessons that are candidates to re-express on `grok/main`.
11. Leave historical successful release lines frozen unless a concrete
   backport requirement is explicitly requested.
