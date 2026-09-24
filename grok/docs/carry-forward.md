# Grok stock adoption

This guide owns the Grok-specific maintainer path for carrying the latest
validated Grok product onto a new stock Codex fixed point. Runtime semantics
stay in [`architecture.md`](./architecture.md), and delivery semantics stay in
[`release.md`](./release.md).

## Branch authority

For the `rust-v0.156.1` adoption:

```text
grok/main
    = latest validated product
    = 82d62fe575707988d23bf629017a26c357714e58

grok/rust-v0.156.1
    = candidate/version line
    = b412ff32c417f855c2b2d1581b77058eed87c84b before this carry-forward merges

carry/grok-rust-v0.156.1
    = reviewable carry-forward work
    = targets grok/rust-v0.156.1
```

Do not move `grok/main` while building or reviewing the candidate. Promotion
of a proven version-line head to `grok/main` is a separate action.

## Fixed points

The current validated Grok semantics come from:

```text
grok/main@82d62fe575707988d23bf629017a26c357714e58
stock base of that product:
openai/codex@40eeb6e8a89ef421c25d4c40e06fa1d40ce66b4f
```

The candidate stock fixed point is:

```text
openai/codex@b412ff32c417f855c2b2d1581b77058eed87c84b
tag: rust-v0.156.1
```

Compare the two stock fixed points before adapting downstream code. A path
without a conflict still needs semantic review, and a nearby stock change does
not by itself justify a Grok patch.

For this adoption, stock `0.156.1` adds the GPT-6 Sol/Luna catalog and picker
migration behavior and changes local agent-message-board lifecycle/query
implementation. Those stock changes stay stock. The Grok product continues to
use `grok/dist/models.json` through stock `model_catalog_json`; catalogs are
not merged and the stock picker is not forked.

## Semantic carry-forward rule

For every current downstream behavior:

```text
Does stock 0.156.1 own it?
```

If yes, use stock and drop the downstream mechanism. Keep only a regression
test when a Grok contract still needs proof.

If no, carry the smallest current Grok semantic at the current stock seam.
Preserve behavior, not the old implementation shape or commit sequence.

Current Grok-specific owners include Provider identity and `ApiDialect::Grok`,
the Responses whitelist, flat-tool projection, hosted calls, Grok SSE
sequencing, encrypted reasoning history projection, Images projection,
Provider-bound App Server/session behavior, the versioned Grok catalog, Facts,
Live, and Actions-native distribution.

Provider-neutral fixes are reviewed the same way. Stock `0.156.1` does not
change the whole-number JSON argument seam or the session input-queue seam, so
those current fixes remain provider-neutral downstream behavior for this
candidate. Small compile/lint consequences of Provider growth stay
provider-neutral rather than becoming Grok conditionals.

## Product boundaries

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

## Procedure

1. Verify `grok/main`, the version line, and the carry branch against the
   fixed SHAs above.
2. Read the current product docs, runtime seams, native tests, Facts, Live, and
   workflow from `grok/main`.
3. Compare stock `40eeb6e8...` to stock
   `b412ff32c417f855c2b2d1581b77058eed87c84b`.
4. Rebuild current semantics on `carry/grok-rust-v0.156.1`, starting from the
   exact version-line stock commit.
5. Fold historical probes, follow-up fixes, schema refreshes, and cleanup into
   the settled semantic owner. Do not recreate removed release, installer, or
   proof-ledger infrastructure.
6. Run native deterministic proof at the owning seams.
7. Open the PR from `carry/grok-rust-v0.156.1` to
   `grok/rust-v0.156.1`.
8. After review and merge, the version-line push builds complete target
   artifacts and runs Linux Live from the exact Linux artifact.
9. Promotion of that proven version-line head to `grok/main` is separate and
   is not part of the carry-forward PR.
