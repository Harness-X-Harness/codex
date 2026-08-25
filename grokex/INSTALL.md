# Install Grokex

The archive contains the Codex `0.149.0` harness with the release-bundled
`grok-4.6` Provider catalog.

## Unix

1. Extract the archive.
2. Run `./install-grokex.sh`.
3. Set `GROK_API_KEY` to a key authorized for the configured Grok endpoint.
4. Run `grokex`.

The installer writes binaries to `${GROKEX_BIN_DIR:-$HOME/.local/bin}` and
copies the profile to `${GROKEX_HOME:-$HOME/.grokex}/config.toml`. It stops if
that configuration file already exists.

## Windows PowerShell

1. Extract the archive.
2. Run `./install-grokex.ps1`.
3. Set `GROK_API_KEY` for your user.
4. Run `grokex.ps1`.

The installer writes binaries to
`$env:LOCALAPPDATA\Grokex\bin` and the profile to
`$env:USERPROFILE\.grokex\config.toml`. It stops if that configuration file
already exists.

## Contract

`wire_api = "grok_responses"` is the only serialized Grok selector. The model
catalog is static release data. The profile does not discover or merge a
remote catalog.
