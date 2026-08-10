# Grokex installation

Grokex is a customized Codex client for the Mini Grok Surface. It installs as
`grokex` and does not replace a stock `codex` installation.

## Install from a release

On macOS or Linux:

```sh
curl -fsSL https://github.com/weavertech-group/codex/releases/latest/download/install-grokex.sh | sh
```

On Windows PowerShell:

```powershell
irm https://github.com/weavertech-group/codex/releases/latest/download/install-grokex.ps1 | iex
```

The default Codex Home is `~/.codex-grok`. The installer creates
`config.toml` from the included example only when the file does not exist.
Set `GROK_API_KEY` in the environment before you start Grokex.

Each archive contains `codex`, `codex-code-mode-host`, this file, the
configuration example, `LICENSE`, and `NOTICE`. Verify manual downloads with
the release `SHA256SUMS` file.
