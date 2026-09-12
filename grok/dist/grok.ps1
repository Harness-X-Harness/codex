$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $env:GROK_HOME) {
    $env:GROK_HOME = Join-Path $env:USERPROFILE ".grok"
}
$env:CODEX_HOME = $env:GROK_HOME
& (Join-Path $ScriptDir "grok-bin.exe") @args
exit $LASTEXITCODE
