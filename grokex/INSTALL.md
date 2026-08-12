# Grokex installation

Grokex is a customized Codex client for ChatGPT and the Mini Grok Surface. It
installs as `grokex` and does not replace a stock `codex` installation.

## Install from a release

On macOS or Linux:

```sh
curl -fsSL https://github.com/weavertech-group/codex/releases/latest/download/install-grokex.sh | sh
```

On Windows PowerShell:

```powershell
irm https://github.com/weavertech-group/codex/releases/latest/download/install-grokex.ps1 | iex
```

The default Codex Home is `~/.codex-grok`. It is one Home for both providers.
The installer creates `config.toml` from the included example only when the
file does not exist. It never replaces an existing configuration or login.

Authenticate each provider through its own path:

```sh
# ChatGPT subscription. This writes the normal ChatGPT login to this Codex Home.
grokex login

# Grok through the Mini Grok Surface. Keep this key outside config.toml.
export GROK_API_KEY="your Mini end-user API key"
```

The model picker lists ChatGPT and Grok models together. A new Thread binds to
the provider that owns its selected model. Existing Threads do not switch
providers. Set `x_search = true` in the Grok provider profile only if you want
to authorize X Search.

Each archive contains `codex`, `codex-code-mode-host`, this file, the
configuration example, `LICENSE`, and `NOTICE`. Verify manual downloads with
the release `SHA256SUMS` file.
