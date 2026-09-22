# Install Grok

The archive contains the Codex harness for this release with the bundled
`grok-4.6` Provider catalog.

Published archives are `x86_64-unknown-linux-musl` (servers and CI) and
`aarch64-apple-darwin` (macOS ARM).

## Unix

1. Extract the archive.
2. Run `./install-grok.sh`.
3. Set `GROK_API_KEY` to a key authorized for the configured Grok endpoint.
4. Run `grok`.

The installer writes binaries to `${GROK_BIN_DIR:-$HOME/.local/bin}` and
copies the profile to `${GROK_HOME:-$HOME/.grok}/config.toml`. It stops if
that configuration file already exists.

Windows binaries are not in the current publication set.

## Contract

`model_provider = "grok"` selects the configured Grok profile. The model
catalog is static release data unless the supported stock config-catalog seam
supplies an explicit replacement. The profile does not discover, merge, or
cache a remote Grok catalog at runtime.
