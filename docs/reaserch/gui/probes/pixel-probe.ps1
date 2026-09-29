<#
.SYNOPSIS
    Prints the exact RGB of individual pixels in a PNG, given "x,y" coordinates.

    Needed to answer one question a bounding box cannot: is a light band *the
    window's own pixels*, or the desktop showing through a transparent window?
    Sampling just inside and just outside the same window edge settles it — if
    the inside is flat light and the outside is dark, the window painted it.

.EXAMPLE
    powershell -File pixel-probe.ps1 -Path shot.png -Points 10,10 20,20
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string[]]$Points,
    [switch]$Grid,
    [int]$GridStep = 20
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$bmp = [System.Drawing.Bitmap]::FromFile((Resolve-Path $Path))
try {
    Write-Host "-- $Path  $($bmp.Width)x$($bmp.Height)"
    foreach ($p in $Points) {
        $parts = $p -split ','
        $x = [int]$parts[0]
        $y = [int]$parts[1]
        if ($x -lt 0 -or $y -lt 0 -or $x -ge $bmp.Width -or $y -ge $bmp.Height) {
            Write-Host ("   ({0,4},{1,4})  out of bounds" -f $x, $y)
            continue
        }
        $c = $bmp.GetPixel($x, $y)
        Write-Host ("   ({0,4},{1,4})  rgb({2,3},{3,3},{4,3})  #{5:X2}{6:X2}{7:X2}" -f $x, $y, $c.R, $c.G, $c.B, $c.R, $c.G, $c.B)
    }

    if ($Grid) {
        Write-Host "   grid (step $GridStep):"
        for ($y = 0; $y -lt $bmp.Height; $y += $GridStep) {
            $row = @()
            for ($x = 0; $x -lt $bmp.Width; $x += $GridStep) {
                $c = $bmp.GetPixel($x, $y)
                $lum = [int](0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B)
                $row += ('{0,3}' -f $lum)
            }
            Write-Host ("   y={0,4}: {1}" -f $y, ($row -join ' '))
        }
    }
}
finally {
    $bmp.Dispose()
}
