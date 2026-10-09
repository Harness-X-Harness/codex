# Install the Grok package

The supported package targets are `x86_64-unknown-linux-musl` and
`aarch64-apple-darwin`. Availability requires an actual successful Actions build
and downloadable artifact. Staging tests are not Linux or macOS package smoke.

Keep the complete extracted directory together:

```text
grok-package.json            # source, target, version and file hashes
codex-package.json           # stock runtime layout metadata
config.toml.example
models.json
INSTALL.md
LICENSE
bin/grok                    # user entrypoint
bin/grok-bin                # actual runtime / app-server entrypoint
bin/codex-code-mode-host
codex-path/rg
codex-resources/zsh/bin/zsh
codex-resources/bwrap        # Linux only
```

The helper/resource locations follow the stock 0.158 package contract. Do not
move the runtime away from its metadata and helpers. The shipped profile and
catalog are the product-owned `grok/dist` files; packaging does not change them.

Use an existing, dedicated absolute `CODEX_HOME`, separate from other products.
The launcher rejects missing/relative Homes, the usual `~/.codex` and `~/.grok`
Homes (including directory aliases), and a Home without `config.toml`. It does
not create a Home, copy configuration, migrate state, or overwrite files.

For a fresh installation, first choose and inspect the dedicated directory.
Copy `config.toml.example` to its `config.toml` and `models.json` beside it only
after confirming those destinations do not contain configuration to preserve.
The profile's relative catalog path resolves beside `config.toml`. Existing
configuration and custom catalogs remain user-owned; compare and merge updates
deliberately. Do not overwrite an unrelated Home.

Restore executable permissions if the download format discarded them:

```sh
chmod +x bin/grok bin/grok-bin bin/codex-code-mode-host codex-path/rg codex-resources/zsh/bin/zsh
[ ! -f codex-resources/bwrap ] || chmod +x codex-resources/bwrap
CODEX_HOME=/absolute/path/to/the/dedicated-home ./bin/grok
```

Supply `GROK_API_KEY` only through an approved execution environment; do not put
it in command arguments or logs. The raw `bin/grok-bin` entrypoint is also used
by explicitly isolated app-server harnesses and does not apply the shell
launcher's Home checks.

`grok-package.json` hashes every other packaged file, including stock metadata.
It records the staging checkout and selected build target, not source acceptance
or an independent attestation of prebuilt executable origin. Actions must bind
the exact checkout, target build, package hash and consuming harness revision.
A diagnostic package is not an accepted release merely because it was staged.
