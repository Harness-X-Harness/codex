# Harness stock adoption

How this Harness line follows stock Codex over time. Runtime semantics stay in
[`architecture.md`](./architecture.md).

Sibling-product branch topology with Grok is owned by
[`docs/downstream-products.md`](../../docs/downstream-products.md). This guide
does not copy that policy.

## Target

```text
exact stock Codex tag
  +
thin Host Goal / workflow integration
  +
small semantic commit stack
  +
native deterministic tests
  +
the unique occupancy/composition Live Story when that composition changed
```

Stock Codex remains the execution system. A Harness change is justified by a
product semantic that stock Codex does not provide.

When a stock-owned integration seam must be changed, keep that seam narrow.
Do not create a generic compatibility, plugin, routing, lifecycle, or
migration framework merely to avoid ordinary Git conflict resolution.

## Semantic commits

The long-lived migration unit is a small set of commits organized by
architecture decisions. A canonical commit should answer:

1. what Harness semantic this commit adds or changes;
2. which stock seam it integrates with;
3. which deterministic tests prove the behavior and stock compatibility.

Tests travel with the behavior they prove. Development history and historical
Issue sequence are not required carry-forward units.

A future stock line is constructed from the new stock tag and the current
semantic commits. Resolve a conflict according to that commit's architecture
responsibility and the new stock behavior. A clean cherry-pick is not
acceptance evidence. A conflict is not by itself an architecture defect.

## Proof

Fork correctness is native tests at the code seam that owns the invariant.
`goal-host-checks` is the deterministic acceptance boundary. Shared stock
seams require stock controls. Product-specific tests must not silently
redefine stock behavior.

Use Live tests only where real composition cannot be fully replaced by
deterministic tests. The unique Live Story asserts occupancy isolation
between Goal HOW and independent `/workflow`. It does not freeze response
counts, timing, or internal event order unless the product contract owns that
invariant.

Git owns version history. Carry-forward does not require a seam ledger,
conflict registry, product-tree identity, or current-delivery ledger.

## Maintainer path

```text
choose exact stock rust-vNEW
  -> create harness/rust-vNEW from that tag
  -> replay/adapt current Harness semantic commits
  -> drop downstream mechanisms stock now owns
  -> push
  -> native Harness CI owns proof
```

Do not merge a new stock tag into an old downstream branch as the common path.
Do not replay complete old branch history. Do not use Grok release/Live state
as a Harness gate. Do not use Mini gitlink, fact, or locator synchronization.

A newer alpha or development tag may be used for disposable rehearsal. It does
not become product authority merely because a rehearsal passes.

## Success

A future maintainer should adopt a new stock Codex line by reading a small
number of current semantic commits, adapting only the seams affected by new
stock behavior, and using executable tests to decide correctness.
