<#
.SYNOPSIS
    Opens (or closes) a large, uniformly dark window in the middle of the
    screen, so that light-coloured window artifacts can be measured against a
    known-dark background.

    Why this exists: "is there a pale box around the card?" is only measurable
    when what is *behind* the card is dark. Several measurements in this
    session were invalid because the app behind had switched to a light theme,
    so the entire crop came back pale and the window's own pixels became
    indistinguishable from the desktop's.

.EXAMPLE
    powershell -File dark-backdrop.ps1            # open it
    powershell -File dark-backdrop.ps1 -Close     # close it again
#>
[CmdletBinding()]
param(
    [switch]$Close,
    [int]$Width = 1500,
    [int]$Height = 820,
    [int]$R = 18,
    [int]$G = 20,
    [int]$B = 24
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public class Backdrop {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern int CreateWindowExW(int ex, string cls, string title, uint style,
        int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr param);
    [DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr h);
    [DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr h, IntPtr dc);
    [DllImport("user32.dll")] public static extern bool UpdateWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);

    public delegate bool EnumProc(IntPtr h, IntPtr p);
    public const uint WM_CLOSE = 0x0010;
    public const uint WS_POPUP = 0x80000000;
    public const uint WS_VISIBLE = 0x10000000;

    public static IntPtr Find(string match) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, p) => {
            var sb = new StringBuilder(256);
            int n = GetWindowTextW(h, sb, sb.Capacity);
            if (sb.ToString(0, n).Contains(match) && IsWindowVisible(h)) { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@

[void][Backdrop]::SetProcessDPIAware()

if ($Close) {
    $h = [Backdrop]::Find('omnitype-dark-backdrop')
    if ($h -eq [IntPtr]::Zero) { Write-Host 'no backdrop open'; exit 0 }
    [void][Backdrop]::PostMessageW($h, [Backdrop]::WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero)
    Write-Host "closed backdrop hwnd=$h"
    exit 0
}

$existing = [Backdrop]::Find('omnitype-dark-backdrop')
if ($existing -ne [IntPtr]::Zero) { Write-Host "backdrop already open (hwnd=$existing)"; exit 0 }

$scr = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$x = [int](($scr.Width - $Width) / 2)
$y = [int](($scr.Height - $Height) / 2)

$h = [Backdrop]::CreateWindowExW(0, 'STATIC', 'omnitype-dark-backdrop',
    [Backdrop]::WS_POPUP -bor [Backdrop]::WS_VISIBLE,
    $x, $y, $Width, $Height, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero)
if ($h -eq [IntPtr]::Zero) { Write-Host "CreateWindowExW failed (err=$([System.Runtime.InteropServices.Marshal]::GetLastWin32Error()))"; exit 1 }

# A bare STATIC window paints COLOR_BTNFACE, so the dark fill is applied by hand.
# GetDC(IntPtr.Zero) is the *screen* DC; pass the window explicitly so the typed
# overload is picked rather than an int-typed one.
$dc = [Backdrop]::GetDC([IntPtr]$h)
$g = [System.Drawing.Graphics]::FromHdc([IntPtr]$dc)
$brush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb($R, $G, $B))
$g.FillRectangle($brush, 0, 0, $Width, $Height)
$g.Dispose()
$brush.Dispose()
[void][Backdrop]::ReleaseDC([IntPtr]$h, [IntPtr]$dc)
[void][Backdrop]::UpdateWindow($h)

Write-Host ("backdrop open: hwnd={0} at ({1},{2}) {3}x{4} fill=rgb({5},{6},{7})" -f $h, $x, $y, $Width, $Height, $R, $G, $B)
Write-Host 'close with: powershell -File dark-backdrop.ps1 -Close'
