# Harness

Harness is a downstream Codex product: stock Codex plus host-owned Goal
completion and an independent Rhai `/workflow` HOW layer.

Current human-readable authorities:

- [Architecture](./docs/architecture.md) — Goal, occupancy, `/workflow`, App Server
- [Carry-forward](./docs/carry-forward.md) — adopting a new stock Codex tag
- [Stories](./docs/stories/) — user-visible composition claims

Implementation stays in the stock Codex modules that own each seam.

Sibling-product branch topology with Grok is owned by
[`docs/downstream-products.md`](../docs/downstream-products.md). This README
does not copy that policy.

Grok Provider semantics and Grok release state are not Harness contracts.

File Harness implementation, Stories, and CI issues in this repository.
Mini proxy issues belong in `ronhuafeng/mini-proxy-core`.
