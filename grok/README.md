# Grok

Grok is a downstream Codex product: stock Codex plus Grok Provider adaptations
at the narrowest backend seams.

Semantic-baseline records:

- [Architecture](./docs/architecture.md) — Grok Provider and host boundary
- [Baseline](./docs/baseline.md) — derivation, semantic stack, and continuation role
- [Request whitelist](./docs/request-whitelist.md) — Grok Responses projection and evidence
- [Stories](./docs/stories/) — user-visible claims and their Rust/Go semantic evidence

Runtime product profile:

- [config.toml.example](./dist/config.toml.example)
- [models.json](./dist/models.json)

This branch is a reconstructed semantic continuation baseline derived from the
accepted `grok/rust-v0.157.1` source. It deliberately excludes binary
packaging, download/install helpers, and workflow-state machinery.

Current product-evolution and binary-distribution policy lives on `grok/main`
and is intentionally not copied into this baseline.
