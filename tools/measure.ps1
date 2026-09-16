# MP14Tools - resource usage check
#
# Launches the built binary, lets it settle, then reports what it costs at idle:
# memory, threads, handles, and - most importantly for battery - how much CPU
# time it accumulates while doing nothing.
#
# Usage:
#   .\tools\measure.ps1                       # release build, 30 s idle sample
#   .\tools\measure.ps1 -Configuration debug -Seconds 60

[CmdletBinding()]
param(
    [ValidateSet('debug', 'release')]
    [string]$Configuration = 'release',
    [int]$Seconds = 30,
    [int]$SettleSeconds = 6
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root "target\$Configuration\mp14tools.exe"

if (-not (Test-Path $exe)) {
    throw "Not built yet: $exe (run .\build.ps1 first)"
}

Write-Host "== Launching $exe" -ForegroundColor Cyan
$process = Start-Process -FilePath $exe -PassThru
Start-Sleep -Seconds $SettleSeconds

if ($process.HasExited) {
    throw "The process exited during startup with code $($process.ExitCode). Check the log."
}

try {
    $process.Refresh()
    $cpuStart = $process.TotalProcessorTime.TotalMilliseconds
    $startedAt = Get-Date

    Write-Host "== Sampling idle CPU for $Seconds s" -ForegroundColor Cyan
    Start-Sleep -Seconds $Seconds

    $process.Refresh()
    $cpuEnd = $process.TotalProcessorTime.TotalMilliseconds
    $elapsed = ((Get-Date) - $startedAt).TotalMilliseconds
    $cores = [Environment]::ProcessorCount
    $cpuPercent = if ($elapsed -gt 0) { (($cpuEnd - $cpuStart) / $elapsed) * 100 / $cores } else { 0 }

    Write-Host ''
    Write-Host '== Results ==' -ForegroundColor Green
    Write-Host ("  working set    : {0:N1} MB" -f ($process.WorkingSet64 / 1MB))
    Write-Host ("  private bytes  : {0:N1} MB" -f ($process.PrivateMemorySize64 / 1MB))
    Write-Host ("  threads        : {0}" -f $process.Threads.Count)
    Write-Host ("  handles        : {0}" -f $process.HandleCount)
    Write-Host ("  GDI objects    : {0}" -f $process.HandleCount)
    Write-Host ("  idle CPU       : {0:N3} % of one core ({1:N0} ms over {2:N0} ms)" -f `
        $cpuPercent, ($cpuEnd - $cpuStart), $elapsed)
    Write-Host ("  total CPU time : {0:N2} s since launch" -f ($cpuEnd / 1000))

    Write-Host ''
    Write-Host '== Child processes ==' -ForegroundColor Green
    $children = Get-CimInstance Win32_Process -Filter "ParentProcessId = $($process.Id)" -ErrorAction SilentlyContinue
    if ($children) {
        $children | Select-Object ProcessId, Name | Format-Table | Out-String | Write-Host
    }
    else {
        Write-Host '  none'
    }
}
finally {
    Write-Host '== Stopping' -ForegroundColor Cyan
    if (-not $process.HasExited) {
        $process.Kill()
        $process.WaitForExit(5000) | Out-Null
    }
}
