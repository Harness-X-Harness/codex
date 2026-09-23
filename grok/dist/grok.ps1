$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

if (-not $env:CODEX_HOME) {
    Write-Error "CODEX_HOME is required for this product. Choose a dedicated product home; do not use the default Codex or Grok home. See ..\INSTALL.md."
    exit 2
}

if ($env:USERPROFILE) {
    $CodexHome = Join-Path $env:USERPROFILE ".codex"
    $GrokHome = Join-Path $env:USERPROFILE ".grok"
    if ($env:CODEX_HOME -eq $CodexHome -or $env:CODEX_HOME -eq $GrokHome) {
        Write-Error "Refusing CODEX_HOME=$env:CODEX_HOME. This product must not share the default Codex (~/.codex) or Grok (~/.grok) home. See ..\INSTALL.md."
        exit 2
    }
}

& (Join-Path $ScriptDir "grok-bin.exe") @args
exit $LASTEXITCODE
