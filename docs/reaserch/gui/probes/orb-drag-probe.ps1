<#
.SYNOPSIS
    Reproduces the pale halo on demand by synthesising a click-drag of the
    orb, then saves before/after full-screen screenshots.

.DESCRIPTION
    The halo does not exist at startup. It appears after the orb has been
    clicked and dragged, it moves and resizes with the orb, and it is absent
    again on a freshly launched instance. That makes the trigger a *window
    move*, not a state change - so it can be reproduced without dictation,
    without the microphone, and without a human at the keyboard.

    The existing orb-ghost-check.ps1 cannot see this artifact: it samples a
    vertical line through the *current* orb and looks for a band just above
    it. A leftover from a previous position is a separate arc somewhere else
    on screen, so this probe saves the raw images and leaves the measuring to
    the analyser, which is told both centres.

.EXAMPLE
    powershell -File orb-drag-probe.ps1
    powershell -File orb-drag-probe.ps1 -Dx 260 -Dy -90 -OutDir C:\temp\drag
#>
[CmdletBinding()]
param(
    # Where to drag, in physical pixels, relative to the orb's centre.
    [int]$Dx = 220,
    [int]$Dy = -80,
    [string]$OutDir = "$env:TEMP\orb-drag",
    [switch]$NoDrag
)

$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public class Drag {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, IntPtr e);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    public delegate bool EnumProc(IntPtr h, IntPtr p);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }

    public const uint LEFTDOWN = 0x0002;
    public const uint LEFTUP   = 0x0004;
    public const uint MOVE    = 0x0001;

    // The orb is untitled and square; the tray helper is untitled but tiny
    // (18x18). Matching on "untitled + square + large" is what the existing
    // click-thief probe already relies on, so the two probes agree on which
    // window is the orb instead of each guessing separately.
    public static IntPtr FindOrb() {
        IntPtr found = IntPtr.Zero; long best = 0;
        EnumWindows((h, p) => {
            if (!IsWindowVisible(h)) return true;
            RECT r; GetWindowRect(h, out r);
            long w = r.Right - r.Left, ht = r.Bottom - r.Top;
            if (w < 150 || w != ht) return true;
            var sb = new StringBuilder(64);
            GetWindowTextW(h, sb, sb.Capacity);
            if (sb.Length != 0) return true;
            if (w > best) { best = w; found = h; }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static string RectOf(IntPtr h) {
        RECT r; GetWindowRect(h, out r);
        return string.Format("{0},{1},{2},{3}", r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top);
    }
}
'@

[void][Drag]::SetProcessDPIAware()

$hwnd = [Drag]::FindOrb()
if ($hwnd -eq [IntPtr]::Zero) {
    Write-Host 'orb window not found - is the app running?'
    exit 2
}
$rect = [Drag]::RectOf($hwnd)
$p = $rect.Split(',')
$cx = [int]$p[0] + [int]$p[2] / 2
$cy = [int]$p[1] + [int]$p[3] / 2
Write-Host ("orb hwnd={0} rect={1} centre=({2},{3})" -f $hwnd, $rect, $cx, $cy)

function Save-Screen([string]$path) {
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.X, $b.Y, 0, 0, $bmp.Size)
    $g.Dispose()
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
}

$before = Join-Path $OutDir 'before.png'
Save-Screen $before
Write-Host "saved $before"

if (-not $NoDrag) {
    # A click-drag in small steps: winit only reports a drag after a few
    # motion events, so one jump can be swallowed.
    [void][Drag]::SetCursorPos($cx, $cy)
    Start-Sleep -Milliseconds 250
    [Drag]::mouse_event([Drag]::LEFTDOWN, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 120
    # Relative moves only: SetCursorPos does not set the MK_LBUTTON bit in the
    # synthesized WM_MOUSEMOVE, so winit never sees a drag and swallows it.
    $steps = 14
    for ($i = 1; $i -le $steps; $i++) {
        [Drag]::mouse_event(
            [Drag]::MOVE,
            [int]($Dx / $steps),
            [int]($Dy / $steps),
            0, [IntPtr]::Zero)
        Start-Sleep -Milliseconds 60
    }
    Start-Sleep -Milliseconds 200
    [Drag]::mouse_event([Drag]::LEFTUP, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 900
}

$after = Join-Path $OutDir 'after.png'
Save-Screen $after
$rect2 = [Drag]::RectOf($hwnd)
$p2 = $rect2.Split(',')
$cx2 = [int]$p2[0] + [int]$p2[2] / 2
$cy2 = [int]$p2[1] + [int]$p2[3] / 2

Write-Host "saved $after"
Write-Host ''
Write-Host ("OLD centre : {0},{1}" -f $cx, $cy)
Write-Host ("NEW centre : {0},{1}" -f $cx2, $cy2)
if ($cx -eq $cx2 -and $cy -eq $cy2) {
    Write-Host 'WARNING: the orb did not move - the drag was not picked up.'
    Write-Host '  The halo may still appear from the click itself; check after.png.'
} else {
    Write-Host ('moved {0} px' -f ([Math]::Sqrt([Math]::Pow($cx2-$cx,2) + [Math]::Pow($cy2-$cy,2))))
}
Write-Host ''
Write-Host 'Analyse after.png around the OLD centre to find any leftover arc.'
exit 0
