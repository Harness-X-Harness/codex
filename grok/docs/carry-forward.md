# Grok stock adoption

This guide owns the Grok-specific maintainer path for moving `grok/main`
onto a new upstream Codex fixed point. Runtime semantics stay in
[`architecture.md`](./architecture.md), and delivery semantics stay in
[`release.md`](./release.md).

## Common path

```text
choose exact upstream SHA
  -> create grok/rust-vX.Y.Z at that SHA
  -> rebuild the current Grok semantic stack on a carry branch
  -> drop downstream mechanisms stock now owns
  -> open the carry PR against grok/rust-vX.Y.Z
  -> Cargo
  -> merge to the version line
  -> complete linux x64 musl + mac arm64 distributions + Linux Live
  -> promote that validated version-line head to grok/main
```

Choosing the upstream fixed point and adapting Grok semantics are
deliberate product work. Do not silently advance the SHA while carrying
forward. If it must move, record the new SHA and re-run the contract
comparison.

## Semantic stack

Carry forward current architectural decisions, not the complete history of
the previous Grok line. Each retained commit should represent a Grok
semantic that stock Codex does not yet provide, together with the native
tests that prove that semantic and the stock seam it changes.

The current candidate line is `grok/rust-v0.156.1`, based on
`openai/codex@b412ff32c417f855c2b2d1581b77058eed87c84b` (`rust-v0.156.1`), and
carried from the validated `grok/main` @
`82d62fe575707988d23bf629017a26c357714e58`. `grok/main` remains on that
validated head until this candidate line passes its push proof.

```text
git log --oneline --reverse b412ff32..HEAD
```

Read oldest first. Each commit is one semantic with its tests, and its body
names the seam and the evidence. No document lists the stack, so nothing
has to be kept in step with it.

When current upstream already owns a downstream mechanism, drop that
mechanism instead of preserving it for history. Current upstream seams to
reuse when they reduce glue:

- `ModelProvider::responses_api_provider(...)`
- `ResolvedResponsesProvider`
- `WorkspaceRoutingContext`
- `ToolPolicy`
- provider `model_catalog_url`

Those seams are not substitutes for Grok wire semantics. Keep the release
`grok/dist/models.json` catalog through stock `model_catalog_json` as the
product authority. Keep the Rust catalog only as the explicit no-catalog
compatibility fallback. Keep request whitelist, flat tools, hosted calls, SSE
sequencing, images, and x_search as dialect-boundary behavior.

Preserve the installation boundary too: each distribution contains
`config.toml.example`, `models.json`, and `INSTALL.md`. The product uses a
dedicated `CODEX_HOME` rather than sharing `~/.codex` or `~/.grok`.

A fix that is independently correct for stock Codex should stay
provider-neutral so it can be upstreamed later. Whole-number JSON integer
arguments are that kind of fix.

### Identity rules that must survive the next adoption

```text
WHO   = model_provider
HOW   = ApiDialect mapped once from WireApi
WHERE = base_url / routing
```

Do not reintroduce:

- `provider.name` dialect selection
- hostname sniffing (`api.x.ai`, TrustedTunnel hosts)
- `GrokModelProvider::api_provider()` mutating display name
- lossy `GrokResponses -> Responses` remote-config conversion
- fail-open search restriction widening
- a second Grok max-edit-image constant

TrustedTunnel remains a transparent transport/evidence path only.

## Procedure

Every step is a command or an existing proof; none needs a new tool.

1. **Choose the SHA.** Fetch the exact upstream commit:
   `git fetch https://github.com/openai/codex b412ff32c417f855c2b2d1581b77058eed87c84b`.
2. **Create the candidate line.** Create `grok/rust-vX.Y.Z` at that exact
   upstream SHA. Keep `grok/main` on the last validated product head.
3. **Rebuild the stack in order.** Create a carry branch from the candidate
   line and port one semantic at a time from the previous Grok line. On
   conflict, resolve at the current stock seam: read what stock now does at
   that seam and keep the Grok semantic on top. A clean cherry-pick is not
   acceptance evidence.
4. **Drop stock-owned mechanisms.** If upstream now owns the behavior,
   delete the downstream copy and keep only a regression that still asserts
   a Grok difference.
5. **Open the PR against the candidate version line.** Cargo runs on the PR.
   Merge when the semantic stack is reviewable.
6. **Prove the version line.** The merge push on `grok/rust-vX.Y.Z` builds
   Linux x64 musl and macOS ARM64 distributions and runs Linux Live.
7. **Promote the validated head.** After the version-line push proof is GREEN,
   move `grok/main` to that exact version-line head. The same commit is both
   the validated candidate result and the new current product head.

Once a candidate is promoted, its `grok/rust-v*` line stays readable as the
version-specific development and proof history. `grok/main` names the latest
validated product head.

## Do not

- Replay the complete previous Grok history as the next stack.
- Merge an old Grok line into a new stock tag.
- Advance the upstream fixed point without recording it.
- Add TrustedTunnel hostname capability profiles.
- Replace the shipped catalog with live `/models`.
- Reintroduce an installer that mutates a user Home.
- Default this product to `~/.codex` or `~/.grok`.
- Reproduce grok-build effort-to-model-id routing inside Codex.
