# MP14Tools - build prerequisite installer
#
# WHY THIS EXISTS
# ---------------
# `eframe`/`egui` pull in the `windows` crate family, and `windows-link` uses
# `#[link(kind = "raw-dylib")]` unconditionally on Windows. For the GNU target,
# rustc generates the import libraries for those raw-dylib links by shelling out
# to `dlltool`, and `dlltool` needs a GNU assembler (`as.exe`).
#
# rustup's `stable-x86_64-pc-windows-gnu` toolchain deliberately ships *no*
# assembler - the `x86_64-w64-mingw32-gcc.exe` in its `self-contained` folder is
# link-only (see its GCC-WARNING.txt). So without an extra MinGW-w64 the build
# always dies with:
#
#     error: dlltool could not create import library ...
#            dlltool.exe: CreateProcess ...
#
# This script installs a user-local MinGW-w64 (WinLibs, no elevated privileges,
# nothing registered in the registry, no service, no autostart entry) and only
# uses it to supply `as.exe`. The linker stays rustup's own toolchain.
#
# Everything it produces goes into <project>\build\toolchain\ - nothing is
# installed system-wide and no registry entry is created.

[CmdletBinding()]
param(
    [string]$InstallDir,
    [string]$AssetPattern = 'winlibs-x86_64-posix-seh-gcc-*-mingw-w64ucrt-*.zip',
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

# Everything this script downloads or extracts stays inside the project folder:
#   build\toolchain\mingw64    the extracted MinGW-w64
#   build\toolchain\assembler  the dlltool + as pair the build actually uses
#   build\toolchain\downloads  the downloaded archive (kept for re-installs)
$toolchainDir = Join-Path (Split-Path -Parent $PSScriptRoot) 'build\toolchain'
if (-not $InstallDir) { $InstallDir = Join-Path $toolchainDir 'mingw64' }

if ((Test-Path (Join-Path $InstallDir 'bin\as.exe')) -and -not $Force) {
    Write-Host "MinGW-w64 already present: $InstallDir" -ForegroundColor Green
    Write-Host 'Use -Force to reinstall.'
    exit 0
}

Write-Host '== Resolving latest WinLibs release' -ForegroundColor Cyan
$release = Invoke-RestMethod `
    -Uri 'https://api.github.com/repos/brechtsanders/winlibs_mingw/releases/latest' `
    -Headers @{ 'User-Agent' = 'mp14tools-setup' }

$asset = $release.assets | Where-Object { $_.name -like $AssetPattern } | Select-Object -First 1
if (-not $asset) {
    throw "No asset matched '$AssetPattern' in release $($release.tag_name)."
}

Write-Host "   release : $($release.tag_name)"
Write-Host "   asset   : $($asset.name) ($([math]::Round($asset.size / 1MB, 1)) MB)"

$tempRoot = Join-Path $toolchainDir 'downloads'
$zipPath = Join-Path $tempRoot $asset.name

if (-not (Test-Path $zipPath)) {
    New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null
    Write-Host '== Downloading' -ForegroundColor Cyan
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zipPath -UseBasicParsing
}
else {
    Write-Host "== Reusing cached download: $zipPath" -ForegroundColor Cyan
}

Write-Host '== Extracting' -ForegroundColor Cyan
$extractRoot = Join-Path $tempRoot 'extract'
if (Test-Path $extractRoot) { Remove-Item $extractRoot -Recurse -Force }
New-Item -ItemType Directory -Force -Path $extractRoot | Out-Null
Expand-Archive -Path $zipPath -DestinationPath $extractRoot -Force

$inner = Get-ChildItem $extractRoot -Directory | Select-Object -First 1
if (-not $inner) { throw 'Archive layout not recognised.' }

$source = Join-Path $inner.FullName 'mingw64'
if (-not (Test-Path $source)) { $source = $inner.FullName }

if (Test-Path $InstallDir) { Remove-Item $InstallDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $InstallDir) | Out-Null
Move-Item $source $InstallDir

# Discard the extracted wrapper folder - only the mingw64 tree is kept.
Remove-Item $extractRoot -Recurse -Force -ErrorAction SilentlyContinue

Write-Host '== Verifying' -ForegroundColor Cyan
$asSource = Join-Path $InstallDir 'bin\as.exe'
& $asSource --version | Select-Object -First 1

# rustc builds the raw-dylib import libraries that `windows-link` demands by
# shelling out to `dlltool`, and dlltool spawns an assembler. Two constraints
# come out of that:
#   1. dlltool does NOT look up `as` through PATH - it calls CreateProcess on a
#      bare "as", which only searches the directory dlltool was loaded from, so
#      `as.exe` and `dlltool.exe` have to live side by side.
#   2. A whole MinGW bin directory must NOT go on PATH: it shadows the linker's
#      CRT search paths and linking then dies with "cannot find crt2.o".
# Hence this small standalone directory holding exactly the tool pair and the
# DLLs those two need.
Write-Host '== Building standalone dlltool/as directory' -ForegroundColor Cyan
$objdump = Join-Path $InstallDir 'bin\objdump.exe'
$assemblerDir = Join-Path (Split-Path -Parent $InstallDir) 'assembler'

if (Test-Path $assemblerDir) { Remove-Item $assemblerDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $assemblerDir | Out-Null

function Get-ImportedDlls([string]$binary) {
    & $objdump -p $binary 2>$null |
        Select-String -Pattern 'DLL Name: (\S+)' |
        ForEach-Object { $_.Matches[0].Groups[1].Value }
}

$systemDll = '^(api-ms-|ext-ms-|KERNEL|USER32|ADVAPI32|SHELL32|OLE32|OLEAUT32|WS2_32|CRYPT32|bcrypt|msvcrt|RPCRT4|GDI32|SHLWAPI|COMCTL32|COMDLG32|USERENV|VERSION|WINMM|ntdll|PSAPI|setupapi|CFGMGR32|dbghelp|POWRPROF)'
$pending = [System.Collections.Generic.Queue[string]]::new()
$seen = [System.Collections.Generic.HashSet[string]]::new()

foreach ($tool in @('as.exe', 'dlltool.exe')) {
    $toolSource = Join-Path $InstallDir "bin\$tool"
    if (-not (Test-Path $toolSource)) { throw "$tool is missing from $InstallDir" }

    Copy-Item $toolSource $assemblerDir
    Get-ImportedDlls $toolSource | ForEach-Object { $pending.Enqueue($_) }
}

while ($pending.Count -gt 0) {
    $name = $pending.Dequeue()
    if (-not $seen.Add($name) -or $name -match $systemDll) { continue }

    $candidate = Join-Path $InstallDir "bin\$name"
    if (-not (Test-Path $candidate)) {
        Write-Warning "dependency not found next to the tools: $name"
        continue
    }

    Copy-Item $candidate $assemblerDir
    Get-ImportedDlls $candidate | ForEach-Object { $pending.Enqueue($_) }
}

# Prove the pair works with a PATH that cannot see the MinGW install: turning a
# .def into a real import library is exactly what rustc asks it to do.
Write-Host '== Verifying the standalone tool pair' -ForegroundColor Cyan
$savedPath = $env:Path
$env:Path = "$env:SystemRoot\System32;$env:SystemRoot"

# The probe directory stays inside build\ so nothing lands in the system temp folder;
# it is deleted again once the check passes.
$probe = Join-Path $toolchainDir '..\temp\dlltool-probe'
if (Test-Path $probe) { Remove-Item $probe -Recurse -Force }
New-Item -ItemType Directory -Force -Path $probe | Out-Null
"LIBRARY kernel32.dll`nEXPORTS`nGetTickCount" | Set-Content (Join-Path $probe 'probe.def') -Encoding ASCII

& (Join-Path $assemblerDir 'as.exe') --version | Select-Object -First 1
& (Join-Path $assemblerDir 'dlltool.exe') -d (Join-Path $probe 'probe.def') -D kernel32.dll -l (Join-Path $probe 'probe.lib') -m i386:x86-64 -f --64 --no-leading-underscore *> (Join-Path $probe 'dlltool.log')
$dlltoolExit = $LASTEXITCODE
$env:Path = $savedPath

$probeLib = Get-Item (Join-Path $probe 'probe.lib') -ErrorAction SilentlyContinue
if ($dlltoolExit -ne 0 -or -not $probeLib -or $probeLib.Length -eq 0) {
    Get-Content (Join-Path $probe 'dlltool.log') -Raw | Write-Host
    throw "The extracted dlltool/as pair cannot build an import library (exit $dlltoolExit)."
}
Remove-Item $probe -Recurse -Force
Write-Host '   import library produced OK' -ForegroundColor Green

$assemblerSize = [math]::Round((Get-ChildItem $assemblerDir | Measure-Object -Property Length -Sum).Sum / 1MB, 2)
Write-Host ''
Write-Host "MinGW-w64     : $InstallDir" -ForegroundColor Green
Write-Host "Tool pair     : $assemblerDir ($assemblerSize MB, first on PATH during builds)" -ForegroundColor Green
Write-Host ''
Write-Host 'Nothing was installed outside the project folder.' -ForegroundColor DarkGray
Write-Host 'To remove all of it:' -ForegroundColor DarkGray
Write-Host "  Remove-Item -Recurse -Force '$toolchainDir'"
