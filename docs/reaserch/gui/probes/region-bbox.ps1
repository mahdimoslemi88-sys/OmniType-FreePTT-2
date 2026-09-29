<#
.SYNOPSIS
    Reports the bounding boxes of two colour classes in a screenshot: "light"
    (any channel >= -Threshold) and "dark" (all channels <= -DarkMax). Used to
    tell a white rectangle that *surrounds* a card (a real window artifact)
    apart from one that is only *offset* from it (a stale window left behind by
    an earlier bubble, with the current transparent card drawn on top).

.EXAMPLE
    powershell -File region-bbox.ps1 -Path shot.png -Threshold 190 -DarkMax 70
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Path,
    [int]$Threshold = 190,
    [int]$DarkMax = 70
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$bmp = [System.Drawing.Bitmap]::FromFile((Resolve-Path $Path))
try {
    $w = $bmp.Width
    $h = $bmp.Height
    Write-Host "-- $Path  ${w}x${h} px"

    function Get-BBox {
        param([scriptblock]$Test)
        $minX = $w; $minY = $h; $maxX = -1; $maxY = -1; $count = 0
        for ($y = 0; $y -lt $h; $y++) {
            for ($x = 0; $x -lt $w; $x++) {
                $c = $bmp.GetPixel($x, $y)
                if (& $Test $c) {
                    $count++
                    if ($x -lt $minX) { $minX = $x }
                    if ($x -gt $maxX) { $maxX = $x }
                    if ($y -lt $minY) { $minY = $y }
                    if ($y -gt $maxY) { $maxY = $y }
                }
            }
        }
        if ($maxX -lt 0) { return $null }
        [pscustomobject]@{
            X = $minX; Y = $minY
            W = $maxX - $minX + 1; H = $maxY - $minY + 1
            Count = $count
        }
    }

    $light = Get-Bbox { param($c) $c.R -ge $Threshold -or $c.G -ge $Threshold -or $c.B -ge $Threshold }
    $dark = Get-Bbox { param($c) $c.R -le $DarkMax -and $c.G -le $DarkMax -and $c.B -le $DarkMax }

    if ($light) {
        Write-Host ("   light bbox = ({0},{1} {2}x{3})  px={4}" -f $light.X, $light.Y, $light.W, $light.H, $light.Count)
    } else {
        Write-Host "   light bbox = none"
    }
    if ($dark) {
        Write-Host ("   dark  bbox = ({0},{1} {2}x{3})  px={4}" -f $dark.X, $dark.Y, $dark.W, $dark.H, $dark.Count)
    } else {
        Write-Host "   dark  bbox = none"
    }

    # Per-row light extent: a *surrounding* box has a flat left and right edge
    # across the rows it covers; an *offset* one is clipped on the side where a
    # newer card is drawn on top of it.
    Write-Host "   row: light-xmin xmax  dark-xmin xmax   (rows with any light px)"
    for ($y = 0; $y -lt $h; $y++) {
        $lx0 = -1; $lx1 = -1; $dx0 = -1; $dx1 = -1
        for ($x = 0; $x -lt $w; $x++) {
            $c = $bmp.GetPixel($x, $y)
            if ($c.R -ge $Threshold -or $c.G -ge $Threshold -or $c.B -ge $Threshold) {
                if ($lx0 -lt 0) { $lx0 = $x }
                $lx1 = $x
            }
            elseif ($c.R -le $DarkMax -and $c.G -le $DarkMax -and $c.B -le $DarkMax) {
                if ($dx0 -lt 0) { $dx0 = $x }
                $dx1 = $x
            }
        }
        if ($lx1 -ge 0) {
            Write-Host ("   {0,4}: L {1,4}..{2,-4}  D {3,4}..{4}" -f $y, $lx0, $lx1, $dx0, $dx1)
        }
    }
}
finally {
    $bmp.Dispose()
}
