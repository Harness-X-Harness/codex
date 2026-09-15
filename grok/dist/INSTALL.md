# Install Grok

The archive contains the Codex harness for this release with the bundled
`grok-4.6` Provider catalog.

## Unix

1. Extract the archive.
2. Run `./install-grok.sh`.
3. Set `GROK_API_KEY` to a key authorized for the configured Grok endpoint.
4. Run `grok`.

The installer writes binaries to `${GROK_BIN_DIR:-$HOME/.local/bin}` and
copies the profile to `${GROK_HOME:-$HOME/.grok}/config.toml`. It stops if
that configuration file already exists.

## Windows PowerShell

1. Extract the archive.
2. Run `./install-grok.ps1`.
3. Set `GROK_API_KEY` for your user.
4. Run `grok.ps1`.

The installer writes binaries to
`$env:LOCALAPPDATA\Grok\bin` and the profile to
`$env:USERPROFILE\.grok\config.toml`. It stops if that configuration file
already exists.

## Contract

`model_provider = "grok"` selects the configured Grok profile. The model
catalog is static release data unless the supported stock config-catalog seam
supplies an explicit replacement. The profile does not discover, merge, or
cache a remote Grok catalog at runtime.
