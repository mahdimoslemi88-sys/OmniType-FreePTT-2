<#
.SYNOPSIS
    Opens a large, uniformly dark window that stays up until closed, so
    light-coloured window artifacts can be measured against a known-dark
    background.

.DESCRIPTION
    Why this exists: "is there a pale band above the orb?" is only measurable
    when what is *behind* the orb is dark. On the light desktop the sampling
    found background luminance 245, and a white artifact against 245 is
    invisible to the sampler — so a "clean" verdict there proves nothing.

    Two reasons this is a separate script rather than the original
    dark-backdrop.ps1:

    1. It paints with raw GDI (CreateSolidBrush + FillRect) instead of
       System.Drawing. PowerShell's overload resolution on
       `Graphics.FromHdc` / `FillRectangle` fails on some hosts with
       "Cannot convert the System.Drawing.Graphics value ... to
       System.Int32", and the window then never appears.

    2. It must survive its own creator. A window created by a PowerShell
       process dies with that process, so the original script could not keep a
       backdrop up after it exited. This one blocks until closed, which is
       what a measurement harness actually needs.

.EXAMPLE
    powershell -File dark-backdrop-hold.ps1              # open, and keep open
    powershell -File dark-backdrop-hold.ps1 -Close       # close it again
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

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public class DarkBackdrop {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern int CreateWindowExW(int ex, string cls, string title, uint style,
        int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr param);
    [DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr h);
    [DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr h, IntPtr dc);
    [DllImport("gdi32.dll")] public static extern IntPtr CreateSolidBrush(uint color);
    [DllImport("gdi32.dll")] public static extern bool DeleteObject(IntPtr o);
    [DllImport("user32.dll")] public static extern bool FillRect(IntPtr dc, ref RECT rc, IntPtr brush);
    [DllImport("user32.dll")] public static extern bool UpdateWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);

    public delegate bool EnumProc(IntPtr h, IntPtr p);
    public const uint WM_CLOSE = 0x0010;
    public const uint WS_POPUP  = 0x80000000;
    public const uint WS_VISIBLE = 0x10000000;
    public const int  SW_SHOWNOACTIVATE = 4;

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

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

    public static uint OwningPid(IntPtr h) {
        uint pid; GetWindowThreadProcessId(h, out pid); return pid;
    }

    public static string Describe(IntPtr h) {
        RECT r; GetWindowRect(h, out r);
        return string.Format("hwnd={0} at ({1},{2}) {3}x{4}", h, r.Left, r.Top,
            r.Right - r.Left, r.Bottom - r.Top);
    }
}
'@

[void][DarkBackdrop]::SetProcessDPIAware()

# Closing is best done by PID: a window cannot be closed from a different
# process, so the opener stays alive and is asked to shut itself down.
$existing = [DarkBackdrop]::Find('omnitype-dark-backdrop')
if ($Close) {
    if ($existing -eq [IntPtr]::Zero) { Write-Host 'no backdrop open'; exit 0 }
    [void][DarkBackdrop]::PostMessageW($existing, [DarkBackdrop]::WM_CLOSE,
        [IntPtr]::Zero, [IntPtr]::Zero)
    Write-Host ("close requested: " + [DarkBackdrop]::Describe($existing))
    exit 0
}

if ($existing -ne [IntPtr]::Zero) {
    Write-Host ("backdrop already open: " + [DarkBackdrop]::Describe($existing))
    Write-Host ("  owner pid=" + [DarkBackdrop]::OwningPid($existing))
    exit 0
}

# Centre on the monitor the orb lives on, then place the window so it covers
# the orb: the artifact being hunted sits a few px above the orb's top edge.
Add-Type -AssemblyName System.Windows.Forms
$scr = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$x = [int](($scr.Width - $Width) / 2)
$y = [int](($scr.Height - $Height) / 2)

$h = [DarkBackdrop]::CreateWindowExW(0, 'STATIC', 'omnitype-dark-backdrop',
    [DarkBackdrop]::WS_POPUP,          # NOT visible yet - see below
    $x, $y, $Width, $Height, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero)
if ($h -eq [IntPtr]::Zero) {
    $err = [System.Runtime.InteropServices.Marshal]::GetLastWin32Error()
    Write-Host "CreateWindowExW failed (err=$err)"
    exit 1
}

# A bare STATIC window paints COLOR_BTNFACE - a light grey, near-white on a
# dark desktop. Creating it with WS_VISIBLE therefore flashes a bright full-
# screen rectangle before the fill below lands, which is exactly the kind of
# pale artifact this probe exists to avoid measuring. So: create it hidden,
# fill it, and only then show it.
# COLOURREF is 0x00BBGGRR, not RGB.
$colour = [uint32](($B -band 0xFF) -shl 16 -bor ($G -band 0xFF) -shl 8 -bor ($R -band 0xFF))
$dc = [DarkBackdrop]::GetDC($h)
$brush = [DarkBackdrop]::CreateSolidBrush($colour)
$rect = New-Object 'DarkBackdrop+RECT'
$rect.Left = 0; $rect.Top = 0; $rect.Right = $Width; $rect.Bottom = $Height
[void][DarkBackdrop]::FillRect($dc, [ref]$rect, $brush)
[void][DarkBackdrop]::ReleaseDC($h, $dc)
[void][DarkBackdrop]::DeleteObject($brush)
[void][DarkBackdrop]::UpdateWindow($h)
[void][DarkBackdrop]::ShowWindow($h, [DarkBackdrop]::SW_SHOWNOACTIVATE)

Write-Host ("backdrop open: " + [DarkBackdrop]::Describe($h) +
    " fill=rgb($R,$G,$B) screen=$($scr.Width)x$($scr.Height)")
Write-Host "holding open; close with:  powershell -File dark-backdrop-hold.ps1 -Close"

# The window belongs to THIS process, so this process must outlive it.
[System.Threading.Thread]::Sleep([System.Threading.Timeout]::Infinite)
