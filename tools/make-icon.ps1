# Generates assets/mp14tools.ico from the same design as src/icon.rs.
#
# The drawing is duplicated here on purpose: a build script cannot use the crate
# it builds, and the alternative - shipping a hand-drawn image - would let the
# three icons drift apart the first time the colour changes. If you change
# `BASE` / `CORNER` / `DOT_RADIUS` in src/icon.rs, change them here too and rerun.
#
# Usage:
#   .\tools\make-icon.ps1

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Drawing

# --- the design, mirroring src/icon.rs -------------------------------------
$baseColour = [System.Drawing.Color]::FromArgb(0xB4, 0xC6, 0xDA)
$dotColour = [System.Drawing.Color]::FromArgb(0xF8, 0xFB, 0xFF)
$corner = 0.24
$dotRadius = 0.14
$samples = 4
$sizes = @(16, 24, 32, 48, 64, 128, 256)

function New-Icon([int]$size) {
    $bitmap = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $side = [double]$size
    $radius = $side * $corner
    $centre = $side / 2.0
    $dot = $side * $dotRadius
    $total = [double]($samples * $samples)

    for ($y = 0; $y -lt $size; $y++) {
        for ($x = 0; $x -lt $size; $x++) {
            $inside = 0
            $dotHits = 0

            for ($sy = 0; $sy -lt $samples; $sy++) {
                for ($sx = 0; $sx -lt $samples; $sx++) {
                    $px = $x + (($sx + 0.5) / $samples)
                    $py = $y + (($sy + 0.5) / $samples)

                    $nearestX = [Math]::Min([Math]::Max($px, $radius), $side - $radius)
                    $nearestY = [Math]::Min([Math]::Max($py, $radius), $side - $radius)
                    $dx = $px - $nearestX
                    $dy = $py - $nearestY
                    if (($dx * $dx + $dy * $dy) -le ($radius * $radius)) {
                        $inside++
                        $dotDx = $px - $centre
                        $dotDy = $py - $centre
                        if (($dotDx * $dotDx + $dotDy * $dotDy) -le ($dot * $dot)) {
                            $dotHits++
                        }
                    }
                }
            }

            if ($inside -eq 0) {
                $bitmap.SetPixel($x, $y, [System.Drawing.Color]::Transparent)
                continue
            }

            $mix = $dotHits / $inside
            $red = [int][Math]::Round($baseColour.R + ($dotColour.R - $baseColour.R) * $mix)
            $green = [int][Math]::Round($baseColour.G + ($dotColour.G - $baseColour.G) * $mix)
            $blue = [int][Math]::Round($baseColour.B + ($dotColour.B - $baseColour.B) * $mix)
            $alpha = [int][Math]::Round(255.0 * $inside / $total)
            $bitmap.SetPixel($x, $y, [System.Drawing.Color]::FromArgb($alpha, $red, $green, $blue))
        }
    }

    return $bitmap
}

# --- ICO container ---------------------------------------------------------
# Every entry is written as a 32-bit DIB rather than a PNG: the shell reads DIB
# entries everywhere, while PNG entries are only honoured by the newer paths.
function Get-DibBytes([System.Drawing.Bitmap]$bitmap) {
    $size = $bitmap.Width
    $stream = New-Object System.IO.MemoryStream
    $writer = New-Object System.IO.BinaryWriter($stream)

    # BITMAPINFOHEADER
    $writer.Write([uint32]40)
    $writer.Write([int32]$size)
    $writer.Write([int32]($size * 2))   # height doubled: XOR bitmap + AND mask
    $writer.Write([uint16]1)
    $writer.Write([uint16]32)
    $writer.Write([uint32]0)            # BI_RGB
    $writer.Write([uint32]($size * $size * 4))
    $writer.Write([int32]0)
    $writer.Write([int32]0)
    $writer.Write([uint32]0)
    $writer.Write([uint32]0)

    # XOR bitmap, bottom-up, BGRA
    for ($y = $size - 1; $y -ge 0; $y--) {
        for ($x = 0; $x -lt $size; $x++) {
            $pixel = $bitmap.GetPixel($x, $y)
            $writer.Write([byte]$pixel.B)
            $writer.Write([byte]$pixel.G)
            $writer.Write([byte]$pixel.R)
            $writer.Write([byte]$pixel.A)
        }
    }

    # AND mask: all zero, the alpha channel does the work.
    $maskRow = [byte[]]::new([int]([Math]::Ceiling($size / 32.0) * 4))
    for ($y = 0; $y -lt $size; $y++) {
        $writer.Write($maskRow)
    }

    $writer.Flush()
    $bytes = $stream.ToArray()
    $writer.Dispose()
    $stream.Dispose()
    # The leading comma keeps PowerShell from unrolling the array into the
    # pipeline, which would turn it into a collection of separate bytes.
    return , $bytes
}

$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $root 'assets\mp14tools.ico'
New-Item -ItemType Directory -Force (Split-Path -Parent $target) | Out-Null

$entries = @()
foreach ($size in $sizes) {
    $bitmap = New-Icon $size
    $entries += [pscustomobject]@{ Size = $size; Bytes = (Get-DibBytes $bitmap) }
    $bitmap.Dispose()
}

$stream = New-Object System.IO.MemoryStream
$writer = New-Object System.IO.BinaryWriter($stream)

# ICONDIR
$writer.Write([uint16]0)                # reserved
$writer.Write([uint16]1)                # type: icon
$writer.Write([uint16]$entries.Count)

# ICONDIRENTRY per image; the offset is relative to the start of the file.
$offset = 6 + 16 * $entries.Count
foreach ($entry in $entries) {
    $dimension = if ($entry.Size -ge 256) { 0 } else { $entry.Size }
    $writer.Write([byte]$dimension)
    $writer.Write([byte]$dimension)
    $writer.Write([byte]0)              # palette size
    $writer.Write([byte]0)              # reserved
    $writer.Write([uint16]1)            # colour planes
    $writer.Write([uint16]32)           # bits per pixel
    $writer.Write([uint32]$entry.Bytes.Length)
    $writer.Write([uint32]$offset)
    $offset += $entry.Bytes.Length
}

foreach ($entry in $entries) {
    # The cast matters: without it PowerShell would treat the array as a single
    # value and write one byte per image.
    $writer.Write([byte[]]$entry.Bytes)
}

$writer.Flush()
[System.IO.File]::WriteAllBytes($target, $stream.ToArray())
$writer.Dispose()
$stream.Dispose()

"sizes : $($sizes -join ', ')"
"written: $target  ($([Math]::Round((Get-Item $target).Length / 1KB, 1)) KB)"
