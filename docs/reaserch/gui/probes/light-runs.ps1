<#
.SYNOPSIS
    Prints, for a screenshot, the contiguous horizontal runs of "light" pixels
    (any channel >= -Threshold) that are at least -MinRun px long, per row.

    A white rectangle painted by a window shows up as one wide run per row with
    the same left/right edges. A white box that is only *partly* covered by a
    newer card on top of it shows up as a run that is clipped on one side — the
    fingerprint of a stale window left behind rather than a live artifact.

.EXAMPLE
    powershell -File light-runs.ps1 -Path shot.png -Threshold 190 -MinRun 20
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Path,
    [int]$Threshold = 190,
    [int]$MinRun = 20,
    [int]$FromY = 0,
    [int]$ToY = -1
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$bmp = [System.Drawing.Bitmap]::FromFile((Resolve-Path $Path))
try {
    $w = $bmp.Width
    $h = $bmp.Height
    if ($ToY -lt 0 -or $ToY -ge $h) { $ToY = $h - 1 }
    Write-Host "-- $Path  ${w}x${h} px   rows $FromY..$ToY   threshold=$Threshold minrun=$MinRun"

    # Cache the image as a byte array: GetPixel per pixel is far too slow for a
    # full-frame scan.
    $rect = [System.Drawing.Rectangle]::new(0, 0, $w, $h)
    $data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $stride = $data.Stride
    $bytes = New-Object byte[] ($stride * $h)
    [System.Runtime.InteropServices.Marshal]::Copy($data.Scan0, $bytes, 0, $bytes.Length)
    $bmp.UnlockBits($data)

    Write-Host "   row : light runs (x0..x1 len)"
    for ($y = $FromY; $y -le $ToY; $y++) {
        $rowBase = $y * $stride
        $runs = @()
        $start = -1
        for ($x = 0; $x -lt $w; $x++) {
            $o = $rowBase + $x * 4
            $b = $bytes[$o]; $g = $bytes[$o + 1]; $r = $bytes[$o + 2]
            $isLight = ($r -ge $Threshold -or $g -ge $Threshold -or $b -ge $Threshold)
            if ($isLight) {
                if ($start -lt 0) { $start = $x }
            }
            else {
                if ($start -ge 0 -and ($x - $start) -ge $MinRun) { $runs += ('{0}..{1}({2})' -f $start, ($x - 1), ($x - $start)) }
                $start = -1
            }
        }
        if ($start -ge 0 -and ($w - $start) -ge $MinRun) { $runs += ('{0}..{1}({2})' -f $start, ($w - 1), ($w - $start)) }
        if ($runs.Count -gt 0) {
            Write-Host ("   {0,4}: {1}" -f $y, ($runs -join '  '))
        }
    }
}
finally {
    $bmp.Dispose()
}
