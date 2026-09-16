# Captures a screenshot of the MP14Tools settings window.
#
# Used to keep the screenshot in the repository README up to date. The window is
# captured from the desktop (the process itself is a normal GPU-composited window,
# so PrintWindow would return a blank frame), which is why the script starts the
# application itself and waits for it to paint.
#
# Usage:
#   .\tools\capture-screenshot.ps1                       # release build
#   .\tools\capture-screenshot.ps1 -Configuration debug
#   .\tools\capture-screenshot.ps1 -Output docs\screenshot-touchpad.png -KeepRunning
#
# An instance that is already running and visible is captured as is; the script
# only starts (and closes) its own instance.

[CmdletBinding()]
param(
    [ValidateSet('debug', 'release')]
    [string]$Configuration = 'release',
    [string]$Output = 'docs\screenshot-touchpad.png',
    # Leave the captured instance running instead of closing it afterwards.
    [switch]$KeepRunning,
    # Milliseconds to wait for the window to appear and paint.
    [int]$SettleMs = 4000
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

Add-Type -Namespace Capture -Name Native -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out RECT rect);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback, IntPtr param);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
[DllImport("shcore.dll")] public static extern int SetProcessDpiAwareness(int awareness);
[StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
public delegate bool EnumProc(IntPtr window, IntPtr param);
'@

<#
.SYNOPSIS
    Largest visible top-level window of a process.
.DESCRIPTION
    MainWindowHandle is not usable here: the process also owns tiny hidden helper
    windows (tray, OSD, notice), and Windows picks whichever it likes - usually
    the wrong one. The settings window is simply the biggest visible one.
#>
function Get-MainWindow {
    param([Parameter(Mandatory)][int]$ProcessId)

    $script:bestWindow = [IntPtr]::Zero
    $script:bestArea = 0

    $callback = [Capture.Native+EnumProc] {
        param($window, $param)

        $owner = 0
        [Capture.Native]::GetWindowThreadProcessId($window, [ref]$owner) | Out-Null
        if ($owner -eq $ProcessId -and [Capture.Native]::IsWindowVisible($window)) {
            $rect = New-Object Capture.Native+RECT
            if ([Capture.Native]::GetWindowRect($window, [ref]$rect)) {
                $area = ($rect.Right - $rect.Left) * ($rect.Bottom - $rect.Top)
                if ($area -gt $script:bestArea) {
                    $script:bestArea = $area
                    $script:bestWindow = $window
                }
            }
        }
        return $true
    }

    [Capture.Native]::EnumWindows($callback, [IntPtr]::Zero) | Out-Null
    return @{ Window = $script:bestWindow; Area = $script:bestArea }
}

# A DPI-unaware process would capture a scaled, blurry bitmap on a HiDPI display.
try { [Capture.Native]::SetProcessDpiAwareness(2) | Out-Null } catch { }

$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root "build\obj\$Configuration\mp14tools.exe"
if (-not (Test-Path -LiteralPath $exe)) {
    throw "Not built yet: $exe. Run .\build.cmd -Release first."
}

# Prefer an instance that is already on screen: it is the one the user is looking
# at, and this script must never close somebody else's process.
$process = @(Get-Process -Name 'mp14tools' -ErrorAction SilentlyContinue) | Select-Object -First 1
$started = $false

if (-not $process) {
    Write-Host "Starting $exe ..."
    $process = Start-Process -FilePath $exe -PassThru
    $started = $true
}
else {
    Write-Host "Using the running instance (pid $($process.Id)) ..."
}

try {
    $window = [IntPtr]::Zero
    for ($attempt = 0; $attempt -lt 40 -and $window -eq [IntPtr]::Zero; $attempt++) {
        Start-Sleep -Milliseconds 250
        $found = Get-MainWindow -ProcessId $process.Id
        # Smaller windows are the tool's own helpers (tray, OSD, notice); the
        # settings window is the only large one.
        if ($found.Area -gt 10000) {
            $window = $found.Window
        }
    }
    if ($window -eq [IntPtr]::Zero) {
        throw 'No visible settings window. Open it from the tray menu first - a window hidden by closing it cannot be captured.'
    }

    # Give Windows a moment to finish the first paint.
    Start-Sleep -Milliseconds $SettleMs
    [Capture.Native]::SetForegroundWindow($window) | Out-Null
    Start-Sleep -Milliseconds 400

    $rect = New-Object Capture.Native+RECT
    if (-not [Capture.Native]::GetWindowRect($window, [ref]$rect)) {
        throw 'GetWindowRect failed.'
    }

    $width = $rect.Right - $rect.Left
    $height = $rect.Bottom - $rect.Top
    if ($width -le 0 -or $height -le 0) {
        throw "The window reported an empty size (${width}x${height})."
    }

    $target = Join-Path $root $Output
    New-Item -ItemType Directory -Force (Split-Path -Parent $target) | Out-Null

    $bitmap = New-Object System.Drawing.Bitmap($width, $height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
    }
    finally {
        $graphics.Dispose()
    }
    try {
        $bitmap.Save($target, [System.Drawing.Imaging.ImageFormat]::Png)
    }
    finally {
        $bitmap.Dispose()
    }

    "captured {0}x{1} -> {2} ({3:N0} KB)" -f $width, $height, $target, ((Get-Item $target).Length / 1KB)
}
finally {
    if ($started -and -not $KeepRunning) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    }
}
