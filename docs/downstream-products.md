# Downstream products

This repository carries two sibling products over stock Codex. Neither product
is based on the other. This document owns only the cross-product branch
topology.

```text
Grok    = exact stock tag + Grok semantic commits
Harness = exact stock tag + Harness semantic commits
```

Canonical branches:

```text
grok/<stock-tag>
harness/<stock-tag>
```

Example: stock `rust-v0.153.4` yields `grok/rust-v0.153.4` and
`harness/rust-v0.153.4`.

## Rules

- Product-only changes stay on their owning product line.
- A fix independently correct for stock Codex should be upstreamed when
  practical and may be replayed into each downstream line that needs it.
- Do not create a permanent `common`, overlay, compatibility, or combined
  downstream branch merely to synchronize products.
- A temporary composition branch is experimental only unless a real third
  product is explicitly defined.
- Grok and Harness commit SHAs need not align.
- Current product docs live only on their owning branch; do not mirror a
  sibling product's current architecture or Stories into the other line.

Product-specific replay details belong in each product's carry-forward
document, not here.

On `grok/<stock-tag>`, start with `grok/README.md`. On
`harness/<stock-tag>`, start with `harness/README.md`. Those branch-local
routers own links to their product architecture, carry-forward guide, and
Stories.

Historical `release/<stock-tag>` names on the Grok line remain historical
evidence. They are not the current Grok canonical ref.

## Issue ownership

```text
Grok implementation / Story / release  -> this repository
Harness implementation / Story / CI    -> this repository
Mini proxy behavior                    -> ronhuafeng/mini-proxy-core
```

Do not file Grok or Harness implementation work in Mini merely because traffic
passes through Mini.
