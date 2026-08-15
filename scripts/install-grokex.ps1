param(
    [string]$Version = "0.148.0-alpha.5",
    [string]$Repository = "Harness-X-Harness/codex",
    [string]$InstallRoot = "$env:LOCALAPPDATA\Grokex",
    [string]$CodexHome = "$HOME\.codex-grok"
)

$ErrorActionPreference = "Stop"

switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64" { $Target = "x86_64-pc-windows-msvc" }
    "ARM64" { $Target = "aarch64-pc-windows-msvc" }
    default { throw "Grokex does not provide an archive for this CPU architecture." }
}

$Tag = "grokex-v$Version"
$AssetName = "grokex-$Target.zip"
$ReleaseUrl = "https://github.com/$Repository/releases/download/$Tag"
$TempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("grokex-" + [guid]::NewGuid())
$ArchivePath = Join-Path $TempRoot $AssetName
$ChecksumsPath = Join-Path $TempRoot "SHA256SUMS"

try {
    New-Item -ItemType Directory -Path $TempRoot -Force | Out-Null
    Invoke-WebRequest "$ReleaseUrl/$AssetName" -OutFile $ArchivePath
    Invoke-WebRequest "$ReleaseUrl/SHA256SUMS" -OutFile $ChecksumsPath

    $ChecksumLine = Get-Content $ChecksumsPath | Where-Object { $_ -match "\s$([regex]::Escape($AssetName))$" }
    if (-not $ChecksumLine) {
        throw "SHA256SUMS does not contain $AssetName."
    }
    $Expected = ($ChecksumLine -split "\s+")[0].ToLowerInvariant()
    $Actual = (Get-FileHash -Algorithm SHA256 $ArchivePath).Hash.ToLowerInvariant()
    if ($Actual -ne $Expected) {
        throw "Checksum verification failed for $AssetName."
    }

    Expand-Archive -Path $ArchivePath -DestinationPath $TempRoot -Force
    $PackageRoot = Join-Path $TempRoot "grokex-$Target"
    if (-not (Test-Path (Join-Path $PackageRoot "bin\codex.exe"))) {
        throw "The Grokex archive does not contain codex.exe."
    }
    if (-not (Test-Path (Join-Path $PackageRoot "bin\codex-code-mode-host.exe"))) {
        throw "The Grokex archive does not contain codex-code-mode-host.exe."
    }

    $VersionRoot = Join-Path $InstallRoot "versions\$Version"
    $CurrentRoot = Join-Path $InstallRoot "current"
    $ShimRoot = Join-Path $InstallRoot "bin"
    New-Item -ItemType Directory -Path (Split-Path $VersionRoot) -Force | Out-Null
    New-Item -ItemType Directory -Path $ShimRoot -Force | Out-Null
    New-Item -ItemType Directory -Path $CodexHome -Force | Out-Null

    if (Test-Path $VersionRoot) { Remove-Item -Recurse -Force $VersionRoot }
    Copy-Item -Recurse -Path $PackageRoot -Destination $VersionRoot
    if (Test-Path $CurrentRoot) { Remove-Item -Recurse -Force $CurrentRoot }
    Copy-Item -Recurse -Path $VersionRoot -Destination $CurrentRoot

    $Shim = Join-Path $ShimRoot "grokex.cmd"
    @"
@echo off
if not defined CODEX_HOME set "CODEX_HOME=%USERPROFILE%\.codex-grok"
"$CurrentRoot\bin\codex.exe" %*
"@ | Set-Content -Path $Shim -Encoding Ascii

    $ConfigPath = Join-Path $CodexHome "config.toml"
    if (-not (Test-Path $ConfigPath)) {
        Copy-Item (Join-Path $VersionRoot "config.toml.example") $ConfigPath
    }

    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $PathParts = @($UserPath -split ";" | Where-Object { $_ })
    if ($PathParts -notcontains $ShimRoot) {
        $NewPath = (@($PathParts) + $ShimRoot) -join ";"
        [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    }

    Write-Host "Installed Grokex $Version as $Shim"
    Write-Host "Open a new terminal, then run 'grokex login' for ChatGPT."
    Write-Host "Set GROK_API_KEY to use Grok, then run: grokex"
}
finally {
    if (Test-Path $TempRoot) { Remove-Item -Recurse -Force $TempRoot }
}
