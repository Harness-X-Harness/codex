# Grok

Grok is a downstream Codex product: stock Codex plus Grok Provider adaptations
at the narrowest backend seams.

Current human-readable authorities:

- [Architecture](./docs/architecture.md) — Grok Provider and harness boundary
- [Stories](./docs/stories/) — user-visible Grok claims and their Rust/Go proof
- [Install](./dist/INSTALL.md) — packaged `grok` command and `~/.grok` home

Release design: [docs/release.md](./docs/release.md).
Stock-adoption procedure belongs in `grok/docs/carry-forward.md` when that
document exists.

Implementation stays in the stock Codex modules that own each seam. This tree
does not relocate Rust for directory symmetry.

Sibling-product branch topology with Harness is owned by
[`docs/downstream-products.md`](../docs/downstream-products.md).

File Grok implementation, Stories, and release issues in this repository.
Mini proxy issues belong in `ronhuafeng/mini-proxy-core`.
