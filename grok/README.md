# Grok

Grok is a downstream Codex product: stock Codex plus Grok Provider adaptations
at the narrowest backend seams.

Current human-readable authorities:

- [Architecture](./docs/architecture.md) — Grok Provider and harness boundary
- [Stories](./docs/stories/) — user-visible Grok claims and their Rust/Go proof

Release and stock-adoption procedures are not this README. They belong in
`grok/docs/release.md` and `grok/docs/carry-forward.md` when those documents
exist.

Implementation stays in the stock Codex modules that own each seam. This tree
does not relocate Rust for directory symmetry.

Sibling-product branch topology with Harness is owned by
[`docs/downstream-products.md`](../docs/downstream-products.md) when that
document exists.
