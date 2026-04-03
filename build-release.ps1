param(
    [switch]$StaticCRT,
    [string]$OutDir = ".\dist",
    [string]$ArtifactName = "OpenCapt_Win_x64",
    [switch]$SkipBuild,
    [switch]$SkipZip
)

$ErrorActionPreference = "Stop"

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location -LiteralPath $scriptRoot

function Resolve-Cargo {
    $cargoFromProfile = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (Test-Path -LiteralPath $cargoFromProfile) {
        return $cargoFromProfile
    }
    return "cargo"
}

if (-not $SkipBuild) {
    $cargo = Resolve-Cargo
    if ($StaticCRT) {
        $env:RUSTFLAGS = "-C target-feature=+crt-static"
    }

    try {
        & $cargo build --release
    }
    finally {
        if ($StaticCRT) {
            Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
        }
    }
}

$exePath = Join-Path $scriptRoot "target\release\opencapt.exe"
if (-not (Test-Path -LiteralPath $exePath)) {
    throw "Build output not found: $exePath"
}

New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
$resolvedOut = (Resolve-Path -LiteralPath $OutDir).Path
$timestamp = Get-Date -Format "yyyyMMdd_HHmmss"
$bundleName = "${ArtifactName}_${timestamp}"
$bundleDir = Join-Path $resolvedOut $bundleName
New-Item -ItemType Directory -Path $bundleDir -Force | Out-Null

$bundleExe = Join-Path $bundleDir "opencapt.exe"
Copy-Item -LiteralPath $exePath -Destination $bundleExe -Force

$readmeLines = @(
    "OpenCapt Quick Start",
    "",
    "1) Run opencapt.exe. The app stays in tray (no main window).",
    "2) Default capture hotkey: Ctrl+Shift+A",
    "3) Config file: %APPDATA%\\OpenCapt\\config.toml",
    "4) If OCR is enabled, set API key and model config in config.toml."
)
$readmeText = $readmeLines -join [Environment]::NewLine
$bundleReadme = Join-Path $bundleDir "README.txt"
[System.IO.File]::WriteAllText($bundleReadme, $readmeText, [System.Text.UTF8Encoding]::new($false))

if (-not $SkipZip) {
    $zipPath = Join-Path $resolvedOut ($bundleName + ".zip")
    if (Test-Path -LiteralPath $zipPath) {
        Remove-Item -LiteralPath $zipPath -Force
    }
    Compress-Archive -Path (Join-Path $bundleDir "*") -DestinationPath $zipPath -Force
    Write-Host "Package generated: $zipPath"
}
else {
    Write-Host "Package directory generated: $bundleDir"
}
