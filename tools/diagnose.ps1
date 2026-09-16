# MP14Tools - toolchain diagnostics
#
# Reports whether the pieces required to build this crate are present:
#   * rustc / cargo
#   * dlltool + a GNU assembler (`as`)  <- required by windows-link's raw-dylib
#   * Windows application control state (Smart App Control / WDAC)

$ErrorActionPreference = 'Continue'

function Write-Section($text) {
    Write-Host ''
    Write-Host "== $text" -ForegroundColor Cyan
}

Write-Section 'rustc / cargo'
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
if (Test-Path $cargo) {
    & $cargo --version
    & (Join-Path $env:USERPROFILE '.cargo\bin\rustc.exe') --version
}
else {
    Write-Host 'cargo not found (install rustup first)' -ForegroundColor Yellow
}

Write-Section 'linker tools'
foreach ($tool in @('dlltool.exe', 'as.exe', 'gcc.exe', 'ld.exe', 'llvm-dlltool.exe')) {
    $found = Get-Command $tool -ErrorAction SilentlyContinue
    if ($found) {
        Write-Host ("  {0,-20} {1}" -f $tool, $found.Source) -ForegroundColor Green
    }
    else {
        Write-Host ("  {0,-20} missing" -f $tool) -ForegroundColor Yellow
    }
}

Write-Section 'application control policy'
try {
    $state = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\CI\Policy' -ErrorAction Stop).VerifiedAndReputablePolicyState
    $label = switch ($state) {
        0 { 'off' }
        1 { 'ENFORCED (Smart App Control on)' }
        2 { 'evaluation' }
        default { "unknown ($state)" }
    }
    Write-Host "  Smart App Control: $label"
}
catch {
    Write-Host '  Smart App Control: not readable'
}

$policies = Get-ChildItem 'C:\Windows\System32\CodeIntegrity\CiPolicies\Active' -ErrorAction SilentlyContinue
Write-Host ("  WDAC active policies: {0}" -f ($policies | Measure-Object).Count)
