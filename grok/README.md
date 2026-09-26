# Grok

Grok is a downstream Codex product: stock Codex plus Grok Provider adaptations
at the narrowest backend seams.

Release-local authorities:

- [Architecture](./docs/architecture.md) — Grok Provider and host boundary
- [Delivery](./docs/release.md) — Actions artifact and proof contract
- [Request whitelist](./docs/request-whitelist.md) — current Grok Responses projection and evidence
- [Stories](./docs/stories/) — user-visible Grok claims and their Rust/Go proof
- [Install](./dist/INSTALL.md) — release assets and dedicated product `CODEX_HOME`

This branch records this release's product and proof state. Current
release-evolution policy is intentionally not copied into the release snapshot.

Implementation stays in the stock Codex modules that own each seam. This tree
does not relocate Rust for directory symmetry.
