# Grokex release

The release line is stock Codex plus the Grok semantic commits. Git owns
history. Native Rust tests own deterministic contracts. `ronhuafeng/llm-go`
owns real-provider Live.

## Checks

`grokex-checks` runs on pull requests to `release/**`:

- `cargo fmt`
- clippy on the crates the graft touches
- the native Grok/stock cargo tests
- packaging helper tests (`release.py`, dist launch scripts)

## Packaging

`grokex-build` compiles target binaries and `grokex/release.py package` lays
out install archives. The version is the `release/rust-v*` branch name.

`grokex/dist/**` is the launch/install surface.

## Live and publish

`grokex-release` builds the six archives, extracts the Linux Codex binary, and
runs `go test -run '^TestGrok'` in `ronhuafeng/llm-go` `codexsdk` against that
binary. `workflow_dispatch` can publish the `grokex-v*` tag. The nightly
schedule never publishes.
