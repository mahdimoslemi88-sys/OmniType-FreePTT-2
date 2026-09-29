<#
.SYNOPSIS
    Screenshots a single window of the running app by its title match, cropped
    to that window's rect, in *physical* screen pixels (DPI aware, so the crop
    lines up with GetWindowRect even at 125% scaling).

    A full-screen grab is not enough here: on a multi-monitor or scaled desktop
    the window rect is in virtual-screen coordinates that do not match a naive
    CopyFromScreen of the primary monitor, and the artifact being measured is
    only a few dozen pixels tall.

.EXAMPLE
    powershell -File capture-preview-window.ps1 -TitleMatch OmniType_Preview -Out shot.png
    powershell -File capture-preview-window.ps1 -TitleMatch OmniType_Preview -Pad 40 -Out card.png
#>
[CmdletBinding()]
param(
    [string]$TitleMatch = 'OmniType_Preview',
    [string]$Out = "$env:TEMP\preview-window.png",
    [int]$Pad = 0,
    [int]$WaitSeconds = 0
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public class CapProbe {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr p);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);

    public static IntPtr Find(uint pid, string match) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, p) => {
            uint wpid;
            GetWindowThreadProcessId(h, out wpid);
            if (wpid != pid) return true;
            var sb = new StringBuilder(512);
            int n = GetWindowTextW(h, sb, sb.Capacity);
            if (sb.ToString(0, n).Contains(match) && IsWindowVisible(h)) { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@

# Per-monitor DPI awareness v2 so GetWindowRect and the bitmap agree at 125%.
try {
    $sig = @'
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
'@
    $u = Add-Type -MemberDefinition $sig -Name 'DpiNative' -Namespace 'Cap' -PassThru
    [void]$u::SetProcessDpiAwarenessContext([IntPtr](-4))
} catch { }

$proc = Get-Process -Name 'voice-ptt' -ErrorAction SilentlyContinue |
    Sort-Object StartTime -Descending | Select-Object -First 1
if (-not $proc) { Write-Host 'voice-ptt is not running.'; exit 1 }

$deadline = (Get-Date).AddSeconds([Math]::Max($WaitSeconds, 0))
$hwnd = [IntPtr]::Zero
do {
    $hwnd = [CapProbe]::Find([uint32]$proc.Id, $TitleMatch)
    if ($hwnd -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 200 }
} while ($hwnd -eq [IntPtr]::Zero -and (Get-Date) -lt $deadline)

if ($hwnd -eq [IntPtr]::Zero) { Write-Host "no visible window matching '$TitleMatch'"; exit 2 }

$r = New-Object CapProbe+RECT
[void][CapProbe]::GetWindowRect($hwnd, [ref]$r)
$w = $r.Right - $r.Left
$h = $r.Bottom - $r.Top
$x = [Math]::Max(0, $r.Left - $Pad)
$y = [Math]::Max(0, $r.Top - $Pad)
$w = $w + 2 * $Pad
$h = $h + 2 * $Pad

Write-Host ("hwnd={0} rect=({1},{2})-({3},{4})  capturing {5}x{6} at ({7},{8})" -f $hwnd, $r.Left, $r.Top, $r.Right, $r.Bottom, $w, $h, $x, $y)

$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
$g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Host "saved $Out"
