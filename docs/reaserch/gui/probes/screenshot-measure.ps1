<#
.SYNOPSIS
  Measure a *saved* screenshot and report every light region in it.

.DESCRIPTION
  The live `artifact-probe.ps1` names the window that owns a band, but only if
  somebody happens to run it while the artifact is on screen. This one works on
  a PNG that already exists (a Snipping-Tool shot, or one of the chat paste
  files), so the measurement can be repeated and diffed after every build.

  It finds connected components of "light" pixels and prints, for each:
  bounding box in image pixels, size, area, and the colours at the four corners
  plus the centre. Sharp edges + a flat/lightly graded interior + saturated blue
  is the signature of a system-composited surface; a run of text or an icon is
  not.

  Sizes are exact pixels, so they can be compared directly against the window
  rects the app logs (`overlay window geometry ... client_w/client_h`).

.EXAMPLE
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File screenshot-measure.ps1 -Path shot.png
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File screenshot-measure.ps1 -Path shot.png -Threshold 200 -MinArea 2000
#>
param(
    [Parameter(Mandatory = $true)][string]$Path,
    [int]$Threshold = 170,
    [int]$MinArea = 400
)

Add-Type -AssemblyName System.Drawing

$full = (Resolve-Path -LiteralPath $Path -ErrorAction SilentlyContinue)
if (-not $full) { Write-Output "file not found: $Path"; exit 1 }
$bmp = [System.Drawing.Bitmap]::FromFile($full.Path)

$w = $bmp.Width
$h = $bmp.Height
Write-Output ("-- {0}  {1}x{2} px" -f $full.Path, $w, $h)
Write-Output ("-- threshold: any channel >= {0}; regions smaller than {1} px ignored" -f $Threshold, $MinArea)
Write-Output ""

# Flat mask, row-major. 1 = light.
$mask = New-Object 'System.Byte[]' ($w * $h)
$lum = New-Object 'System.Int32[]' ($w * $h)
for ($y = 0; $y -lt $h; $y++) {
    $row = $y * $w
    for ($x = 0; $x -lt $w; $x++) {
        $c = $bmp.GetPixel($x, $y)
        $i = $row + $x
        $lum[$i] = [int](($c.R + $c.G + $c.B) / 3)
        if ($c.R -ge $Threshold -or $c.G -ge $Threshold -or $c.B -ge $Threshold) { $mask[$i] = 1 }
    }
}

$seen = New-Object 'System.Byte[]' ($w * $h)
$stack = New-Object 'System.Collections.Generic.Stack[int]'
$regions = @()

for ($y0 = 0; $y0 -lt $h; $y0++) {
    for ($x0 = 0; $x0 -lt $w; $x0++) {
        $start = $y0 * $w + $x0
        if ($mask[$start] -eq 0 -or $seen[$start] -eq 1) { continue }

        # Flood fill this component.
        $seen[$start] = 1
        $stack.Push($start)
        $area = 0
        $minX = $w; $maxX = -1; $minY = $h; $maxY = -1
        $sumR = 0; $sumG = 0; $sumB = 0

        while ($stack.Count -gt 0) {
            $p = $stack.Pop()
            # Floor, not round: a plain `[int]($p / $w)` cast rounds to nearest
            # and walks off the edge of the bitmap.
            $py = [int][math]::Floor($p / $w)
            $px = $p - ($py * $w)
            $c = $bmp.GetPixel($px, $py)
            $area++
            $sumR += $c.R; $sumG += $c.G; $sumB += $c.B
            if ($px -lt $minX) { $minX = $px }
            if ($px -gt $maxX) { $maxX = $px }
            if ($py -lt $minY) { $minY = $py }
            if ($py -gt $maxY) { $maxY = $py }

            if ($px -gt 0)     { $n = $p - 1;      if ($mask[$n] -eq 1 -and $seen[$n] -eq 0) { $seen[$n] = 1; $stack.Push($n) } }
            if ($px -lt $w - 1) { $n = $p + 1;      if ($mask[$n] -eq 1 -and $seen[$n] -eq 0) { $seen[$n] = 1; $stack.Push($n) } }
            if ($py -gt 0)     { $n = $p - $w;      if ($mask[$n] -eq 1 -and $seen[$n] -eq 0) { $seen[$n] = 1; $stack.Push($n) } }
            if ($py -lt $h - 1) { $n = $p + $w;      if ($mask[$n] -eq 1 -and $seen[$n] -eq 0) { $seen[$n] = 1; $stack.Push($n) } }
        }

        if ($area -ge $MinArea) {
            $regions += [pscustomobject]@{
                X = $minX; Y = $minY; W = $maxX - $minX + 1; H = $maxY - $minY + 1; Area = $area
                R = [int]($sumR / $area); G = [int]($sumG / $area); B = [int]($sumB / $area)
            }
        }
    }
}

$bmp.Dispose()

# Biggest first: the artifact is always bigger than the icons it sits next to.
$regions = $regions | Sort-Object Area -Descending

Write-Output ("-- {0} light region(s) found" -f $regions.Count)
foreach ($r in $regions) {
    Write-Output ("   rect=({0},{1} {2}x{3})  area={4}  fill=({5},{6},{7})" -f $r.X, $r.Y, $r.W, $r.H, $r.Area, $r.R, $r.G, $r.B)
}
