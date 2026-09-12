# Grok stock adoption

This guide owns the Grok-specific maintainer path for moving to a new stock
Codex tag. Runtime semantics stay in [`architecture.md`](./architecture.md),
and delivery semantics stay in [`release.md`](./release.md).

Sibling-product branch topology is owned by
[`docs/downstream-products.md`](../../docs/downstream-products.md). This guide
does not copy that cross-product policy.

## Common path

```text
choose exact stock rust-vNEW
  -> create grok/rust-vNEW from that tag
  -> replay/adapt the current Grok semantic commits
  -> drop downstream mechanisms stock now owns
  -> push
  -> normal Grok CI builds, runs Go Live, and publishes grok-vNEW
```

Choosing the stock tag and adapting Grok semantics are deliberate product
work. Release choreography is not a separate maintainer step.

## Semantic stack

Carry forward current architectural decisions, not the complete history of the
old downstream branch. Each retained commit should represent a Grok semantic
that stock Codex does not yet provide, together with the native tests that
prove that semantic and the stock seam it changes.

When new stock Codex already owns a downstream mechanism, drop that mechanism
instead of preserving it for history. Resolve ordinary conflicts according to
the current stock seam and the Grok architecture; a clean cherry-pick is not
acceptance evidence, and a conflict is not by itself an architecture defect.

A fix that is independently correct for stock Codex should be upstreamed when
practical. Until then it may be replayed as a stock-compatible fix rather than
being coupled to Grok-only behavior.

## Proof and publication

The first push on a new Grok line and every later push use the same delivery
path:

```text
Rust native checks
  -> six target builds
  -> Go native Grok Live on the exact Linux artifact
  -> moving grok-v<stock-version> publication
  -> minimum GitHub readback
```

There is no rehearsal release, `dry_run`, empty-destination preparation,
manual publication phase, Mini gitlink/fact/locator synchronization, or
separate freeze transaction.

When development moves from `grok/rust-vOLD` to `grok/rust-vNEW`, stop pushing
the old line. Its last GREEN `grok-vOLD` state is then historical naturally.

## Boundaries

- Do not merge a new stock tag into an old Grok branch as the common path.
- Do not replay complete old branch history when current semantic commits are
  sufficient.
- Do not use Harness branch, CI, or release state as Grok acceptance.
- Do not use Mini as a Grok source or publication coordinator.
- Do not add branch-policy parsers, documentation validators, release ledgers,
  or another migration framework for this procedure.
