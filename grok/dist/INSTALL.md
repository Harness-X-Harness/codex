# Install the Grok package

The supported package targets are `x86_64-unknown-linux-musl` and
`aarch64-apple-darwin`. Availability requires an actual successful Actions build
and downloadable artifact. Staging tests are not Linux or macOS package smoke.

The complete Linux package is built and smoke-tested on Ubuntu 24.04. The
`musl` target describes the Rust runtime. The broader Ubuntu 20.04+/Debian 10+
support statement in stock `docs/install.md` has not been verified for this
complete Grok package.

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
bin/codex-exec-server
codex-path/rg
codex-resources/bwrap        # Linux only
```

The helper/resource locations follow the stock package contract introduced in
0.165. Do not move the runtime away from its metadata and helpers. The shipped
profile and catalog are the product-owned `grok/dist` files; packaging does not
change them.

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
chmod +x bin/grok bin/grok-bin bin/codex-code-mode-host bin/codex-exec-server codex-path/rg
[ ! -f codex-resources/bwrap ] || chmod +x codex-resources/bwrap
CODEX_HOME=/absolute/path/to/the/dedicated-home ./bin/grok
```

Supply `GROK_API_KEY` only through an approved execution environment; do not put
it in command arguments or logs. The raw `bin/grok-bin` entrypoint is also used
by explicitly isolated app-server harnesses and does not apply the shell
launcher's Home checks.

## One App Server with OpenAI and Grok threads

The packaged runtime is this fork's Codex binary with both OpenAI and Grok
wire-dialect support; it is not an official stock OpenAI binary. An App Server
client can use one runtime process and one Home configuration for threads bound
to either configured provider. This does not require or replace the existing
Grok-only `config.toml.example`.

For example, in a **dedicated** Home, a unified configuration can use stock
OpenAI as its default and define Grok alongside it:

```toml
model_provider = "openai"

[model_providers.grok]
name = "Grok"
base_url = "https://grok.trustedtunnel.app/v1"
env_key = "GROK_API_KEY"
wire_api = "grok_responses"
requires_openai_auth = false
supports_websockets = false
```

Use the existing OpenAI authentication mechanism and provide `GROK_API_KEY`
through an approved execution environment. Neither credential belongs in this
file, command arguments, or logs. Do not copy the Grok-only example's top-level
`model_catalog_json` or Grok-specific `[memories]` model overrides into this
unified example: those are global settings, not per-provider settings. The
runtime's Grok compatibility catalog supplies `grok-4.6` without a catalog
file. This is distinct from the bundled Grok-only profile and its `models.json`
(which lists `grok-4.7` and `grok-4.6`).

After starting `bin/grok app-server` with this Home and initializing the App
Server, a client selects both provider and model on each `thread/start`, for
example with these `params` objects:

```json
{"modelProvider":"openai","model":"<an available OpenAI model>"}
```

```json
{"modelProvider":"grok","model":"grok-4.6"}
```

Use an available OpenAI model from the default provider's `model/list` response
in place of the placeholder. `model/list` remains a view of the configured
default provider, not a merged cross-provider catalog: when OpenAI is the
default it does not advertise Grok. A client must therefore know the configured
Grok provider ID and supported model, rather than infer them from `model/list`.
A thread retains its provider for subsequent turns and resume; select a different
provider by starting another thread, not by switching a turn. This App Server
contract does not imply that every CLI, TUI, or SDK picker can select both
providers in one session.

For credential-free startup diagnostics, use the stock one-shot entrypoint:

```sh
CODEX_HOME=/absolute/path/to/the/dedicated-home ./bin/grok app-server --print-startup-config
```

It validates configuration and prints requirements or a structured error,
without starting the App Server runtime. `--startup-client-info` accepts stock
ClientInfo JSON when a host needs to supply its identity. Configuration or
organization-policy errors remain failures; this command is not a policy
bypass, a backend test, or proof that a Desktop settings popup is resolved.

`grok-package.json` hashes every other packaged file, including stock metadata.
It records the staging checkout and selected build target, not source acceptance
or an independent attestation of prebuilt executable origin. Actions must bind
the exact checkout, target build, package hash and consuming harness revision.
A diagnostic package is not an accepted release merely because it was staged.
