$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $env:GROKEX_HOME) {
    $env:GROKEX_HOME = Join-Path $env:USERPROFILE ".grokex"
}
$env:CODEX_HOME = $env:GROKEX_HOME
& (Join-Path $ScriptDir "grokex-bin.exe") @args
exit $LASTEXITCODE
