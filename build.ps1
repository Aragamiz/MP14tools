# MP14Tools - toolchain environment bootstrap
#
# Puts the Rust GNU toolchain and the project-local dlltool/as pair on PATH so
# rustc can resolve them, then forwards to cargo.
#
# All generated files live under build\ (see .cargo\config.toml for the cargo
# target directory). Deleting build\ removes every generated file.
#
# Usage:
#   .\build.ps1            # debug build
#   .\build.ps1 -Release   # release build
#   .\build.ps1 -Run       # debug build + run
#
# A running instance locks its own executable, and Windows then refuses to
# replace it - cargo fails with "Access denied" while linking. The script stops
# any instance of the configuration it is about to build, unless -KeepRunning is
# given.

[CmdletBinding()]
param(
    [switch]$Release,
    [switch]$Run,
    [switch]$Check,
    # Build even when an instance of this configuration is still running.
    [switch]$KeepRunning
)

$ErrorActionPreference = 'Stop'

$toolchain = Join-Path $env:USERPROFILE '.rustup\toolchains\stable-x86_64-pc-windows-gnu'
$rustlibBin = Join-Path $toolchain 'lib\rustlib\x86_64-pc-windows-gnu\bin'

$pathParts = @()

# The dlltool/as pair must come first: dlltool resolves its assembler from the
# directory it was loaded from, and rustc must pick this dlltool rather than the
# assembler-less one shipped with the GNU toolchain.
$toolPair = Join-Path $PSScriptRoot 'build\toolchain\assembler'
if (Test-Path (Join-Path $toolPair 'dlltool.exe')) {
    $pathParts += $toolPair
}

$pathParts += @(
    (Join-Path $env:USERPROFILE '.cargo\bin'),
    (Join-Path $rustlibBin 'self-contained'),
    $rustlibBin
)

$env:Path = ($pathParts + $env:Path) -join ';'

if (-not (Get-Command 'as.exe' -ErrorAction SilentlyContinue)) {
    Write-Warning 'The dlltool/as pair is missing. Run tools\install-mingw.ps1 first, or the build will fail inside dlltool.'
}

$configuration = if ($Release) { 'release' } else { 'debug' }
$outputExe = Join-Path $PSScriptRoot "build\obj\$configuration\mp14tools.exe"

<#
.SYNOPSIS
    Stop instances of mp14tools that are running from the build output.
.DESCRIPTION
    A process keeps its image file open, so the linker cannot overwrite it. Only
    processes running from this exact executable are touched - an instance
    started from somewhere else is left alone, and the build is allowed to fail
    with cargo's own message in that case.
#>
function Stop-StaleInstance {
    param([Parameter(Mandatory)][string]$Exe)

    if (-not (Test-Path -LiteralPath $Exe)) { return }

    $stale = @(
        Get-Process -Name 'mp14tools' -ErrorAction SilentlyContinue |
            Where-Object { try { $_.Path -eq $Exe } catch { $false } }
    )
    if ($stale.Count -eq 0) { return }

    Write-Host "Stopping $($stale.Count) running mp14tools instance(s) that lock the output..."
    $stale | Stop-Process -Force -ErrorAction SilentlyContinue

    # Give Windows a moment to release the file handles.
    Start-Sleep -Milliseconds 400
}

if (-not $Check -and -not $KeepRunning) {
    Stop-StaleInstance -Exe $outputExe
}

Push-Location $PSScriptRoot
try {
    $cargoArgs = @()
    if ($Release) { $cargoArgs += '--release' }

    if ($Check) {
        & cargo check @cargoArgs
    }
    elseif ($Run) {
        & cargo run @cargoArgs
    }
    else {
        & cargo build @cargoArgs
    }

    $code = $LASTEXITCODE

    if ($code -eq 0 -and -not $Check) {
        Write-Host "Output: $outputExe"
    }

    exit $code
}
finally {
    Pop-Location
}
