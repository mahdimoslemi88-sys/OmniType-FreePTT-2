<#
.SYNOPSIS
  OmniType FreePTT window-artifact probe: find the pale "bar"/"box" that appears
  above the orb or around the transcript card and name the window that owns it.

.DESCRIPTION
  The reported artifact: a light blue-white rectangle that shows up just above
  the orb (and a white box around the transcript card). Its colour in the user's
  screenshots measures ~(224,241,255) at the left edge fading to ~(221,237,254),
  i.e. a white surface with a blue tint, on a dark desktop.

  This script answers "whose pixels are those?" instead of guessing:

    1. finds the app's orb window (class `Window Class`, visible) and the
       transcript window (title `OmniType_Preview`, when it exists),
    2. captures the screen around them and reports every horizontal band of
       light pixels (rows with many light pixels), with the exact colours at the
       band's left edge / middle / right edge,
    3. for the middle of each band, lists every visible top-level window that
       contains that point **in z-order** (EnumWindows order = top to bottom) with
       pid, class, title, rect and GWL_STYLE/GWL_EXSTYLE bits.

  The first entry in that z-order list is the window whose pixels are on screen
  there — that is the answer (our orb window, our preview window, or a
  third-party overlay).

  Read-only: EnumWindows/GetWindow* + a screen capture. It changes nothing.

.EXAMPLE
  # run WHILE the light bar/box is on screen:
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File artifact-probe.ps1
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File artifact-probe.ps1 -Pad 160 -SaveCapture bar.png
#>
param(
    [int]$TargetPid = 0,
    [int]$Pad = 120,
    [int]$MinLightRun = 60,
    [string]$SaveCapture = ""
)

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public static class OmniArtifactProbe
{
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern int GetWindowLongW(IntPtr hWnd, int index);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    public static string FindWindow(uint pid, string clsOrTitle, bool visibleOnly, bool biggest)
    {
        string res = "";
        int bestArea = 0;
        EnumWindows(delegate(IntPtr h, IntPtr l)
        {
            uint p;
            GetWindowThreadProcessId(h, out p);
            if (p != pid) return true;
            if (visibleOnly && !IsWindowVisible(h)) return true;
            var cls = new StringBuilder(256);
            GetClassNameW(h, cls, cls.Capacity);
            var title = new StringBuilder(256);
            GetWindowTextW(h, title, title.Capacity);
            bool hit = cls.ToString() == clsOrTitle || title.ToString().Contains(clsOrTitle);
            if (!hit) return true;
            RECT r;
            GetWindowRect(h, out r);
            int area = (r.Right - r.Left) * (r.Bottom - r.Top);
            if (!biggest || area > bestArea)
            {
                bestArea = area;
                res = string.Format("{0},{1},{2},{3}", r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top);
                if (!biggest) return false;
            }
            return true;
        }, IntPtr.Zero);
        return res;
    }

    public static List<string> WindowsAtPoint(int px, int py)
    {
        var found = new List<string>();
        int z = 0;
        EnumWindows(delegate(IntPtr h, IntPtr l)
        {
            z++;
            if (!IsWindowVisible(h)) return true;
            RECT r;
            GetWindowRect(h, out r);
            if (px >= r.Left && px < r.Right && py >= r.Top && py < r.Bottom)
            {
                uint p;
                GetWindowThreadProcessId(h, out p);
                var cls = new StringBuilder(256);
                GetClassNameW(h, cls, cls.Capacity);
                var title = new StringBuilder(256);
                GetWindowTextW(h, title, title.Capacity);
                int style = GetWindowLongW(h, -16);
                int ex = GetWindowLongW(h, -20);
                bool caption = (style & 0x00C00000) != 0;
                bool thick = (style & 0x00040000) != 0;
                bool popup = (style & unchecked((int)0x80000000)) != 0;
                bool layered = (ex & 0x00080000) != 0;
                found.Add(string.Format(
                    "z={0,-4} pid={1,-6} rect=({2},{3} {4}x{5}) style=0x{6:X8} ex=0x{7:X8} caption={8,-5} thickframe={9,-5} popup={10,-5} layered={11,-5} class={12} title=\"{13}\"",
                    z, p, r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top, style, ex,
                    caption, thick, popup, layered, cls.ToString(), title.ToString()));
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
"@

function Get-AppProcess {
    if ($TargetPid -gt 0) { return Get-Process -Id $TargetPid -ErrorAction SilentlyContinue }
    return Get-Process -Name voice-ptt -ErrorAction SilentlyContinue | Select-Object -First 1
}

function Scan-Region {
    param([int]$X, [int]$Y, [int]$W, [int]$H, [string]$Label)

    if ($W -le 0 -or $H -le 0) { return }
    if ($X -lt 0) { $X = 0 }
    if ($Y -lt 0) { $Y = 0 }

    $bmp = New-Object System.Drawing.Bitmap($W, $H)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($X, $Y, 0, 0, (New-Object System.Drawing.Size($W, $H)))
    $g.Dispose()

    if ($SaveCapture) {
        $shot = $bmp.Clone()
        $shot.Save($SaveCapture, [System.Drawing.Imaging.ImageFormat]::Png)
        $shot.Dispose()
        Write-Output ("-- capture saved to {0} (region {1},{2} {3}x{4})" -f $SaveCapture, $X, $Y, $W, $H)
    }

    Write-Output ("== {0}: region ({1},{2}) {3}x{4}" -f $Label, $X, $Y, $W, $H)

    $inBand = $false
    $band = @{}
    $bands = 0
    for ($yy = 0; $yy -lt $H; $yy++) {
        $n = 0; $mn = 99999; $mx = -1; $sr = 0; $sg = 0; $sb = 0
        for ($xx = 0; $xx -lt $W; $xx++) {
            $c = $bmp.GetPixel($xx, $yy)
            # "light" = bright and bluish-or-neutral; the artifact is a white
            # surface with a blue tint, so b >= r >= g is a good discriminator.
            if ($c.R -ge 185 -and $c.G -ge 185 -and $c.B -ge 185) {
                $n++
                if ($xx -lt $mn) { $mn = $xx }
                if ($xx -gt $mx) { $mx = $xx }
                $sr += $c.R; $sg += $c.G; $sb += $c.B
            }
        }
        $isBand = ($n -ge 20) -and (($mx - $mn) -ge $MinLightRun)
        if ($isBand -and -not $inBand) {
            $band = @{ y0 = $yy; y1 = $yy; x0 = $mn; x1 = $mx; n = $n; sr = $sr; sg = $sg; sb = $sb }
            $inBand = $true
        } elseif ($isBand -and $inBand) {
            $band.y1 = $yy
            if ($mn -lt $band.x0) { $band.x0 = $mn }
            if ($mx -gt $band.x1) { $band.x1 = $mx }
            $band.sr += $sr; $band.sg += $sg; $band.sb += $sb; $band.n += $n
        } elseif (-not $isBand -and $inBand) {
            Report-Band $bmp $X $Y $band
            $bands++
            $inBand = $false
        }
    }
    if ($inBand) { Report-Band $bmp $X $Y $band; $bands++ }
    if ($bands -eq 0) { Write-Output "   (no light band found in this region)" }
    $bmp.Dispose()
}

function Report-Band {
    param($Bmp, [int]$X, [int]$Y, $band)

    $screenY0 = $Y + $band.y0
    $screenY1 = $Y + $band.y1
    $screenX0 = $X + $band.x0
    $screenX1 = $X + $band.x1
    $midY = [int](($band.y0 + $band.y1) / 2)
    $cL = $Bmp.GetPixel($band.x0, $midY)
    $cM = $Bmp.GetPixel([int](($band.x0 + $band.x1) / 2), $midY)
    $cR = $Bmp.GetPixel($band.x1 - 1, $midY)
    Write-Output ("   band: screen y={0}..{1} (h={2}) x={3}..{4} (w={5})" -f $screenY0, $screenY1, ($screenY1 - $screenY0 + 1), $screenX0, $screenX1, ($screenX1 - $screenX0 + 1))
    Write-Output ("         colours left/mid/right = ({0}) ({1}) ({2})" -f $cL.ToString(), $cM.ToString(), $cR.ToString())

    $px = [int](($screenX0 + $screenX1) / 2)
    $py = [int](($screenY0 + $screenY1) / 2)
    Write-Output ("         owner windows at centre ({0},{1}), z-order top to bottom:" -f $px, $py)
    foreach ($line in [OmniArtifactProbe]::WindowsAtPoint($px, $py)) {
        Write-Output ("            {0}" -f $line)
    }
}

$proc = Get-AppProcess
if (-not $proc) {
    Write-Output "voice-ptt.exe is not running."
    exit 1
}

Write-Output ("-- pid={0} private={1} MB   {2}" -f $proc.Id, [math]::Round($proc.PrivateMemorySize64 / 1MB, 1), (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Write-Output "-- run this WHILE the light bar/box is visible; the z-order list names the owner"
Write-Output ""

$orb = [OmniArtifactProbe]::FindWindow([uint32]$proc.Id, "Window Class", $true, $true)
if ($orb) {
    $q = $orb -split ','
    $x = [int]$q[0]; $y = [int]$q[1]; $w = [int]$q[2]; $h = [int]$q[3]
    Write-Output ("-- orb window (class 'Window Class') = ({0},{1} {2}x{3})" -f $x, $y, $w, $h)
    Scan-Region ($x - $Pad) ($y - $Pad) ($w + 2 * $Pad) ($h + 2 * $Pad) "around orb window"
} else {
    Write-Output "-- no visible 'Window Class' window found"
}

$prev = [OmniArtifactProbe]::FindWindow([uint32]$proc.Id, "OmniType_Preview", $false, $false)
if ($prev) {
    $q = $prev -split ','
    $x = [int]$q[0]; $y = [int]$q[1]; $w = [int]$q[2]; $h = [int]$q[3]
    Write-Output ""
    Write-Output ("-- transcript window (title 'OmniType_Preview') = ({0},{1} {2}x{3})" -f $x, $y, $w, $h)
    Scan-Region ($x - 48) ($y - 48) ($w + 96) ($h + 96) "around transcript window"
} else {
    Write-Output ""
    Write-Output "-- no 'OmniType_Preview' window exists right now (the transcript card only"
    Write-Output "   exists for ~10 s after a transcription; dictate first, then re-run)"
}
