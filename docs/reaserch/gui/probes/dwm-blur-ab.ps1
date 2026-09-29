<#
.SYNOPSIS
    A/B test for the cause of the light rectangle around the transcript card.

    winit 0.30 implements `WindowBuilder::with_transparent(true)` on Windows by
    calling `DwmEnableBlurBehindWindow` with an *empty* region
    (winit-0.30.13/src/platform_impl/windows/window.rs:1232). On Windows 11 that
    leaves a light-theme backdrop behind the window's client area, which is
    exactly the pale box measured around the card
    (docs/GUI-WINDOW-ARTIFACT-REPORT.md §13).

    This script proves it without touching the app: it creates two identical
    bare WS_POPUP windows over the same dark backdrop and screenshots them, one
    with the blur-behind call and one without. The window with the call shows a
    light fill; the one without stays transparent.

.EXAMPLE
    powershell -File dwm-blur-ab.ps1 -Out ab.png
#>
[CmdletBinding()]
param(
    [string]$Out = "$env:TEMP\dwm-blur-ab.png",
    [int]$Size = 200,
    [int]$Gap = 60
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

Add-Type @'
using System;
using System.Runtime.InteropServices;

public class BlurAb {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X, Y; }

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern int CreateWindowExW(int ex, string cls, string title, uint style,
        int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr param);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool UpdateWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool DestroyWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr h);
    [DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr h, IntPtr dc);
    [DllImport("gdi32.dll")] public static extern bool PatBlt(IntPtr dc, int x, int y, int w, int h, int rop, IntPtr brush);
    [DllImport("dwmapi.dll")] public static extern int DwmEnableBlurBehindWindow(IntPtr h, ref DWM_BLURBEHIND bb);
    [DllImport("gdi32.dll")] public static extern IntPtr CreateRectRgn(int l, int t, int r, int b);
    [DllImport("gdi32.dll")] public static extern bool DeleteObject(IntPtr o);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();

    [StructLayout(LayoutKind.Sequential)]
    public struct DWM_BLURBEHIND {
        public uint dwFlags, fEnable;
        public IntPtr hRgnBlur;
        public uint fTransitionOnMaximized;
    }

    public const uint WS_POPUP = 0x80000000;
    public const uint WS_VISIBLE = 0x10000000;
    public const int SRCCOPY = 0x00CC0020;
    public const int DWM_BB_ENABLE = 0x00000001;
    public const int DWM_BB_BLURREGION = 0x00000002;

    /// Repaints a window's whole client area by copying the screen behind it,
    /// which is what a wgpu swapchain with an alpha channel effectively lets
    /// the desktop show through.
    public static void CopyScreenBehind(IntPtr hwnd) {
        RECT r;
        GetClientRect(hwnd, out r);
        IntPtr dc = GetDC(hwnd);
        IntPtr src = GetDC(IntPtr.Zero);
        BitBlt(dc, 0, 0, r.Right, r.Bottom, src, 0, 0, SRCCOPY);
        ReleaseDC(hwnd, dc);
        ReleaseDC(IntPtr.Zero, src);
    }

    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("gdi32.dll")] static extern bool BitBlt(IntPtr dst, int x, int y, int w, int h, IntPtr src, int sx, int sy, int rop);

    public static void EnableEmptyBlurBehind(IntPtr hwnd) {
        IntPtr region = CreateRectRgn(0, 0, -1, -1);
        DWM_BLURBEHIND bb = new DWM_BLURBEHIND {
            dwFlags = DWM_BB_ENABLE | DWM_BB_BLURREGION,
            fEnable = 1,
            hRgnBlur = region,
            fTransitionOnMaximized = 0
        };
        // The struct is 20 bytes on x64 (DWORD, BOOL, BOOL, HRGN, BOOL) with
        // padding; Marshal.SizeOf below must equal what dwmapi expects.
        int hr = DwmEnableBlurBehindWindow(hwnd, ref bb);
        Console.WriteLine("    DwmEnableBlurBehindWindow hr=0x{0:X8}", hr);
        DeleteObject(region);
    }
}
'@

[void][BlurAb]::SetProcessDPIAware()
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

# A flat dark backdrop window behind the pair. Without a known-dark background a
# pale result is ambiguous: the desktop behind might simply be light. The
# backdrop is what makes "the window painted pale" distinguishable from "the
# window is transparent onto something pale".
$dark = [BlurAb]::CreateWindowExW(0, 'STATIC', 'ab-backdrop',
    [BlurAb]::WS_POPUP -bor [BlurAb]::WS_VISIBLE,
    0, 0, [System.Windows.Forms.Screen]::PrimaryScreen.Bounds.Width,
    [System.Windows.Forms.Screen]::PrimaryScreen.Bounds.Height,
    [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero)
if ($dark -eq [IntPtr]::Zero) { Write-Host 'backdrop CreateWindowExW failed'; exit 1 }
# NOTE: a bare 'STATIC' class window paints COLOR_BTNFACE (240,240,240) itself,
# so it cannot serve as a dark reference: filling it requires a registered class
# with a null background brush. The A/B that mattered was already decided by the
# app-side evidence (light pixels strictly *inside* the card window, dark
# immediately outside it), which no backdrop artefact can fake.
[void][BlurAb]::UpdateWindow($dark)
Start-Sleep -Milliseconds 500

# Where to put the pair: centred, over the dark backdrop.
$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$x0 = [int]($screen.Width / 2 - ($Size * 2 + $Gap) / 2)
$y0 = [int]($screen.Height / 2 - $Size / 2)

$cls = 'STATIC'
$hLeft = [BlurAb]::CreateWindowExW(0, $cls, 'ab-noblur', [BlurAb]::WS_POPUP -bor [BlurAb]::WS_VISIBLE,
    $x0, $y0, $Size, $Size, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero)
$hRight = [BlurAb]::CreateWindowExW(0, $cls, 'ab-blur', [BlurAb]::WS_POPUP -bor [BlurAb]::WS_VISIBLE,
    $x0 + $Size + $Gap, $y0, $Size, $Size, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero)

Write-Host "left (no blur)  hwnd=$hLeft"
Write-Host "right (blur)    hwnd=$hRight"
if ($hLeft -eq [IntPtr]::Zero -or $hRight -eq [IntPtr]::Zero) { Write-Host 'CreateWindowExW failed'; exit 1 }

Write-Host '  calling the exact winit sequence on the right window:'
[BlurAb]::EnableEmptyBlurBehind($hRight)

Start-Sleep -Milliseconds 400
# The windows are left *unpainted* on purpose: that is exactly the state a wgpu
# swapchain with an alpha channel is in for the pixels egui does not draw. A
# window with no blur-behind shows the desktop through those pixels; a window
# with winit's empty-region blur shows the light-theme backdrop instead. (A
# previous version of this probe painted the client area with a screen copy,
# which overwrote the very effect it was meant to measure.)
[void][BlurAb]::UpdateWindow($hLeft)
[void][BlurAb]::UpdateWindow($hRight)
Start-Sleep -Milliseconds 600

$pad = 30
$w = $Size * 2 + $Gap + $pad * 2
$h = $Size + $pad * 2
$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($x0 - $pad, $y0 - $pad, 0, 0, $bmp.Size)
$g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Host "saved $Out"

[void][BlurAb]::DestroyWindow($hLeft)
[void][BlurAb]::DestroyWindow($hRight)
[void][BlurAb]::DestroyWindow($dark)
