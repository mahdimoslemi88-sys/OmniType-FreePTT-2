<#
.SYNOPSIS
    Detects the "ghost arc" above the orb: pixels left on screen by a click
    region that shrank without DWM recompositing.

.DESCRIPTION
    The orb is one fixed 298x298 window whose click region grows and shrinks
    with the animation - 58 pt idle, 96 pt while Recording. DWM composites a
    per-pixel-alpha window from a cached redirection surface, and
    `SetWindowRgn` on its own does not invalidate that surface, so a shrink
    left a hard-edged arc of the *old* circle floating above the orb with
    nothing painted in it. Measured on the artifact: a 9 px band, hard edges,
    concentric with the orb, radius 120.4 px - which is
    `painted_radius_pt(Recording)` = 96.25 pt x 1.25 to within 0.1 px.

    This samples the vertical centre line of the orb window and flags any row
    band that is far brighter than the background, has a hard edge, is thin,
    and lies outside the orb itself. A soft glow fails the hard-edge test; the
    orb itself fails the distance test.

    Run it right after a dictation ends. Exit code 1 means the ghost is there.

.EXAMPLE
    powershell -File orb-ghost-check.ps1
    powershell -File orb-ghost-check.ps1 -Save C:\temp\after-dictation.png
    powershell -File orb-ghost-check.ps1 -Shot C:\temp\shot.png
#>
[CmdletBinding()]
param(
    # Analyse this PNG instead of grabbing a fresh screenshot.
    [string]$Shot,

    # Orb window rect as "x,y,w,h" in physical pixels. Required with -Shot,
    # since the live window has usually moved by the time an old screenshot is
    # analysed; click-thief-probe.ps1 prints it.
    [string]$Rect,

    # Save the screenshot that was analysed, for the record.
    [string]$Save,

    # Minimum luminance above the background for a band to count. 60 is well
    # under the 190 the artifact measured and well over the ~10 a soft glow
    # contributes.
    [double]$Threshold = 60
)

$ErrorActionPreference = 'Stop'

# Without this the probe reports coordinates divided by the scale factor on a
# 125% display, which is the bug that invalidated the numbers in the older
# sections of the artifact report.
try {
    Add-Type -Name DpiCtx -Namespace Probe -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
'@
    [Probe.DpiCtx]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
} catch {
    Write-Verbose "could not set per-monitor-v2 DPI awareness: $_"
}

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public class GhostProbe {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr p);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);

    public class Win { public IntPtr Hwnd; public int X, Y, W, H; }

    // The orb is the only top-level window of the process big enough to be a
    // 298px overlay; the tray message window is 18px and the card is gone.
    public static List<Win> ForProcess(uint want) {
        var result = new List<Win>();
        EnumWindows((h, _) => {
            uint pid;
            GetWindowThreadProcessId(h, out pid);
            if (pid != want) return true;
            if (!IsWindowVisible(h)) return true;
            RECT r;
            if (!GetWindowRect(h, out r)) return true;
            int w = r.Right - r.Left, ht = r.Bottom - r.Top;
            if (w < 100 || ht < 100) return true;
            result.Add(new Win { Hwnd = h, X = r.Left, Y = r.Top, W = w, H = ht });
            return true;
        }, IntPtr.Zero);
        return result;
    }
}
'@

$proc = Get-Process voice-ptt -ErrorAction SilentlyContinue
$orb = $null
if ($Rect) {
    $n = $Rect.Split(',')
    $orb = [pscustomobject]@{
        Hwnd = [IntPtr]0; X = [int]$n[0]; Y = [int]$n[1]; W = [int]$n[2]; H = [int]$n[3]
    }
} elseif ($proc) {
    $wins = [GhostProbe]::ForProcess([uint32]$proc.Id)
    if ($wins.Count -gt 0) { $orb = $wins[0] }
}
if (-not $orb) {
    Write-Output 'orb window not found - pass -Rect "x,y,w,h" for an offline screenshot'
    exit 2
}

if ($Shot) {
    $bmp = [System.Drawing.Bitmap]::FromFile($Shot)
} else {
    $screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap $screen.Width, $screen.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($screen.Location, [System.Drawing.Point]::Empty, $screen.Size)
    $g.Dispose()
    if ($Save) { $bmp.Save($Save) }
}

try {
    $cx = [int]($orb.X + $orb.W / 2)
    $cy = [int]($orb.Y + $orb.H / 2)
    $top = $orb.Y + 2

    if ($Shot -and -not $Rect) {
        Write-Output '-Shot without -Rect cannot tell the orb from any other bright UI on screen.'
        Write-Output 'Pass -Rect "x,y,w,h" (physical px, from click-thief-probe.ps1).'
        exit 2
    }
    Write-Output ("orb hwnd=0x{0:X} rect {1},{2} {3}x{4}  centre {5},{6}" -f `
        $orb.Hwnd.ToInt64(), $orb.X, $orb.Y, $orb.W, $orb.H, $cx, $cy)

    # Luminance down the centre line, from the top of the window to its middle.
    $lum = New-Object 'System.Collections.Generic.List[double]'
    for ($y = $top; $y -le $cy; $y++) {
        $c = $bmp.GetPixel($cx, $y)
        $lum.Add(($c.R + $c.G + $c.B) / 3.0)
    }

    # Background = the median of the 20 rows immediately above the window's top
    # edge, at the same column. That is guaranteed to be whatever is behind the
    # orb, with no orb and no artifact in it. Estimating it from inside the
    # window instead fails whenever the orb is in a bright state, because the
    # "background" comes out brighter than the artifact and nothing can trip.
    $behind = New-Object 'System.Collections.Generic.List[double]'
    for ($y = [Math]::Max(0, $top - 10); $y -lt $top; $y++) {
        $c = $bmp.GetPixel($cx, $y)
        $behind.Add([double](($c.R + $c.G + $c.B) / 3.0))
    }
    if ($behind.Count -eq 0) {
        for ($y = 0; $y -lt [int]($lum.Count / 2); $y++) {
            if (($cy - ($top + $y)) -lt 90) { continue }
            $c = $bmp.GetPixel($cx, $y)
            $behind.Add([double](($c.R + $c.G + $c.B) / 3.0))
        }
    }
    $sorted = [double[]]$behind.ToArray()
    [Array]::Sort($sorted)
    $bg = [double]$sorted[[int]($sorted.Length / 2)]
    Write-Output ("background {0}  (window {1}x{2}px)" -f $bg, $orb.W, $orb.H)

    $cut = $bg + $Threshold
    $found = @()
    $i = 0
    while ($i -lt $lum.Count) {
        if ($lum[$i] -le $cut) { $i++; continue }
        $s = $i
        while ($i -lt $lum.Count -and $lum[$i] -gt $cut) { $i++ }
        $e = $i - 1
        $height = $e - $s + 1
        $peak = ($lum[$s..$e] | Measure-Object -Maximum).Maximum

        $midY = $top + ($s + $e) / 2
        $dist = [int]($cy - $midY)          # distance above the orb centre
        $hard = if ($s -gt 0) { ($lum[$s - 1] - $bg) -lt 30 } else { $false }
        $isOrb = $dist -lt 90               # the orb and its glow

        # The artifact is an *arc of a circle* concentric with the orb, so the
        # radius implied by its top edge and the radius implied by its bottom
        # edge have to agree. A line of text, a toolbar or a scrollbar in the
        # window behind does not, which is what keeps the probe honest when the
        # orb happens to sit over a bright one.
        $rTop = 0.0; $rBot = 0.0
        foreach ($edge in @(@($s, 'top'), @($e, 'bot'))) {
            $idx = $edge[0]; $y = $top + $idx
            $d = $cy - $y
            $half = 0
            while (($cx - $half - 1) -ge 0) {
                $c = $bmp.GetPixel($cx - $half - 1, $y)
                if (($c.R + $c.G + $c.B) / 3.0 -le $cut) { break }
                $half++
            }
            $rr = [Math]::Sqrt($d * $d + $half * $half)
            if ($edge[1] -eq 'top') { $rTop = $rr } else { $rBot = $rr }
        }
        $isArc = [Math]::Abs($rTop - $rBot) -le 4.0

        if ($height -ge 4 -and $height -le 24 -and $hard -and -not $isOrb -and $isArc) {
            $found += [pscustomobject]@{
                Rows   = "{0}..{1}" -f ($top + $s), ($top + $e)
                Px     = $height
                Dist   = $dist
                Rise   = [int]($peak - $bg)
                Radius = "{0:N1}/{1:N1}" -f $rTop, $rBot
            }
        }
    }

    if ($found.Count -eq 0) {
        Write-Output 'VERDICT: clean - no ghost arc above the orb'
        exit 0
    }
    Write-Output 'VERDICT: GHOST ARC PRESENT'
    Write-Output ($found | Format-Table -AutoSize | Out-String).Trim()
    exit 1
} finally {
    $bmp.Dispose()
}
