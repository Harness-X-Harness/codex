$ErrorActionPreference = "Stop"
$ArchiveRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$BinDir = if ($env:GROKEX_BIN_DIR) { $env:GROKEX_BIN_DIR } else { Join-Path $env:LOCALAPPDATA "Grokex\bin" }
$GrokexHome = if ($env:GROKEX_HOME) { $env:GROKEX_HOME } else { Join-Path $env:USERPROFILE ".grokex" }
$ConfigPath = Join-Path $GrokexHome "config.toml"

if (Test-Path $ConfigPath) {
    throw "Refusing to overwrite $ConfigPath"
}

New-Item -ItemType Directory -Force -Path $BinDir, $GrokexHome | Out-Null
Copy-Item (Join-Path $ArchiveRoot "bin\grokex.ps1") (Join-Path $BinDir "grokex.ps1")
Copy-Item (Join-Path $ArchiveRoot "bin\grokex-bin.exe") (Join-Path $BinDir "grokex-bin.exe")
Copy-Item (Join-Path $ArchiveRoot "bin\codex-code-mode-host.exe") (Join-Path $BinDir "codex-code-mode-host.exe")
Copy-Item (Join-Path $ArchiveRoot "config.toml.example") $ConfigPath

Write-Host "Installed Grokex in $BinDir. Set GROK_API_KEY, then run grokex.ps1."
