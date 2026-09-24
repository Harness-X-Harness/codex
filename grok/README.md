# Grok

Grok is a downstream Codex product: stock Codex plus Grok Provider adaptations
at the narrowest backend seams.

Current human-readable authorities:

- [Architecture](./docs/architecture.md) — Grok Provider and host boundary
- [Delivery](./docs/release.md) — Actions artifact and proof contract
- [Carry-forward](./docs/carry-forward.md) — adopting a new upstream fixed point onto `grok/main`
- [Stories](./docs/stories/) — user-visible Grok claims and their Rust/Go proof
- [Install](./dist/INSTALL.md) — release assets and the dedicated product `CODEX_HOME` contract

Implementation stays in the stock Codex modules that own each seam. This tree
does not relocate Rust for directory symmetry.

File Grok implementation, Stories, and release issues in this repository.
Mini proxy issues belong in `ronhuafeng/mini-proxy-core`.
