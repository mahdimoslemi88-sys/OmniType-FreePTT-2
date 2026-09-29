<#
.SYNOPSIS
    Enumerates every top-level window of the running voice-ptt process whose
    title contains "OmniType_Preview" and dumps its screen rect, DPI and
    window styles.

    A screenshot alone cannot tell a *live* white box from a *stale* one: two
    overlapping preview windows look identical in a picture. This probe counts
    the windows instead, which is the difference between "the card's own window
    paints white" and "an old card's window was never destroyed and is still on
    screen".

.EXAMPLE
    powershell -File preview-window-probe.ps1
    powershell -File preview-window-probe.ps1 -Watch -Seconds 30
#>
[CmdletBinding()]
param(
    [switch]$Watch,
    [int]$Seconds = 10,
    [int]$IntervalMs = 500
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public class WinProbe {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr p);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr64(IntPtr h, int i);
    [DllImport("user32.dll", EntryPoint = "GetWindowLongW")] public static extern int GetWindowLong32(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowPlacement(IntPtr h, ref WINDOWPLACEMENT lp);

    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)]
    public struct WINDOWPLACEMENT {
        public int length, flags, showCmd;
        public POINT ptMinPosition, ptMaxPosition;
        public RECT rcNormalPosition;
    }

    public static IntPtr GetWindowLongPtr(IntPtr h, int i) {
        return IntPtr.Size == 8 ? GetWindowLongPtr64(h, i) : new IntPtr(GetWindowLong32(h, i));
    }

    public static List<IntPtr> ProcessWindows(uint pid) {
        var list = new List<IntPtr>();
        EnumWindows((h, p) => {
            uint wpid;
            GetWindowThreadProcessId(h, out wpid);
            if (wpid == pid) list.Add(h);
            return true;
        }, IntPtr.Zero);
        return list;
    }

    public static string Title(IntPtr h) {
        var sb = new StringBuilder(512);
        int n = GetWindowTextW(h, sb, sb.Capacity);
        return sb.ToString(0, n);
    }

    [DllImport("dwmapi.dll")]
    public static extern int DwmGetWindowAttribute(IntPtr h, int attr, out int value, int size);

    /// DWMWA_WINDOW_CORNER_PREFERENCE = 33
    public static int GetCornerPref(IntPtr h) { int v; int hr = DwmGetWindowAttribute(h, 33, out v, 4); return hr == 0 ? v : -1; }
    /// DWMWA_BORDER_COLOR = 34
    public static int GetBorderColor(IntPtr h) { int v; int hr = DwmGetWindowAttribute(h, 34, out v, 4); return hr == 0 ? v : -1; }
    /// DWMWA_SYSTEMBACKDROP_TYPE = 38
    public static int GetBackdrop(IntPtr h) { int v; int hr = DwmGetWindowAttribute(h, 38, out v, 4); return hr == 0 ? v : -1; }
    /// DWMWA_NCRENDERING_POLICY = 2
    public static int GetNcPolicy(IntPtr h) { int v; int hr = DwmGetWindowAttribute(h, 2, out v, 4); return hr == 0 ? v : -1; }
    /// DWMWA_CAPTION_BOUNDS = 31
    public static int GetCaptionHeight(IntPtr h) {
        int v; int hr = DwmGetWindowAttribute(h, 31, out v, 4);
        return hr == 0 ? v : -1;
    }

    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);

    /// DWMWA_EXTENDED_FRAME_BOUNDS = 9
    public static bool GetExtendedFrame(IntPtr h, out RECT r) {
        return DwmGetWindowAttributeRect(h, 9, out r) == 0;
    }
    [DllImport("dwmapi.dll", EntryPoint = "DwmGetWindowAttribute")]
    public static extern int DwmGetWindowAttributeRect(IntPtr h, int attr, out RECT r);
}
'@

$styleNames = @{
    0x80000000 = 'WS_POPUP'; 0x00C00000 = 'WS_CAPTION'; 0x00080000 = 'WS_SYSMENU'
    0x00020000 = 'WS_MINIMIZEBOX'; 0x00010000 = 'WS_MAXIMIZEBOX'; 0x00040000 = 'WS_THICKFRAME'
    0x00800000 = 'WS_BORDER'; 0x00400000 = 'WS_DLGFRAME'; 0x00000080 = 'WS_CHILD'
    0x10000000 = 'WS_VISIBLE'; 0x00000010 = 'WS_CLIPSIBLINGS'
}
# NOTE: 0x00040000 is both WS_EX_TOPMOST and the numeric twin of the bit below;
# a hashtable cannot hold it twice, so ex-style names are matched in bit order.
$exStyleNames = @{
    0x00000080 = 'WS_EX_TOOLWINDOW'; 0x00000100 = 'WS_EX_WINDOWEDGE'
    0x00000200 = 'WS_EX_CLIENTEDGE'; 0x00020000 = 'WS_EX_STATICEDGE'
    0x00040000 = 'WS_EX_TOPMOST'; 0x00080000 = 'WS_EX_LAYERED'
    0x00200000 = 'WS_EX_COMPOSITED'; 0x02000000 = 'WS_EX_NOREDIRECTIONBITMAP'
}

function Format-Style([long]$bits, $map) {
    $names = @()
    foreach ($k in $map.Keys) { if (($bits -band $k) -eq $k -and $k -ne 0) { $names += $map[$k] } }
    if ($names.Count -eq 0) { return '(none)' }
    return ($names -join '|')
}

function Get-AppPid {
    $p = Get-Process -Name 'voice-ptt' -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 -or $true }
    if (-not $p) { return $null }
    return ($p | Sort-Object StartTime -Descending | Select-Object -First 1).Id
}

function Show-Windows([int]$pid_) {
    $handles = [WinProbe]::ProcessWindows([uint32]$pid_)
    Write-Host "== pid $pid_ : $($handles.Count) top-level window(s)"
    $n = 0
    foreach ($h in $handles) {
        $t = [WinProbe]::Title($h)
        $vis = [WinProbe]::IsWindowVisible($h)
        $r = New-Object WinProbe+RECT
        [void][WinProbe]::GetWindowRect($h, [ref]$r)
        $style = [WinProbe]::GetWindowLongPtr($h, -16).ToInt64()
        $ex = [WinProbe]::GetWindowLongPtr($h, -20).ToInt64()
        $dpi = [WinProbe]::GetDpiForWindow($h)
        $wp = New-Object WinProbe+WINDOWPLACEMENT
        $wp.length = [System.Runtime.InteropServices.Marshal]::SizeOf([type]'WinProbe+WINDOWPLACEMENT')
        [void][WinProbe]::GetWindowPlacement($h, [ref]$wp)
        $winW = $r.Right - $r.Left
        $winH = $r.Bottom - $r.Top
        $n++
        Write-Host ("  [{0}] hwnd={1} vis={2} dpi={3} showCmd={4}" -f $n, $h, $vis, $dpi, $wp.showCmd)
        Write-Host ("      title   = '{0}'" -f $t)
        Write-Host ("      rect    = ({0},{1})-({2},{3})  {4}x{5}" -f $r.Left, $r.Top, $r.Right, $r.Bottom, $winW, $winH)
        Write-Host ("      style   = 0x{0:X8}  {1}" -f $style, (Format-Style $style $styleNames))
        Write-Host ("      exstyle = 0x{0:X8}  {1}" -f $ex, (Format-Style $ex $exStyleNames))
        Write-Host ("      dwm     = ncPolicy={0} backdrop={1} cornerPref={2} borderColor=0x{3:X8} captionH={4}" -f `
            [WinProbe]::GetNcPolicy($h), [WinProbe]::GetBackdrop($h), [WinProbe]::GetCornerPref($h), `
            [WinProbe]::GetBorderColor($h), [WinProbe]::GetCaptionHeight($h))
        $cr = New-Object WinProbe+RECT
        [void][WinProbe]::GetClientRect($h, [ref]$cr)
        $clientW = $cr.Right - $cr.Left
        $clientH = $cr.Bottom - $cr.Top
        $org = New-Object WinProbe+POINT
        [void][WinProbe]::ClientToScreen($h, [ref]$org)
        Write-Host ("      client  = {0}x{1}  at ({2},{3})   nonclient = {4}x{5}" -f `
            $clientW, $clientH, $org.X, $org.Y, ($winW - $clientW), ($winH - $clientH))
        $ef = New-Object WinProbe+RECT
        if ([WinProbe]::GetExtendedFrame($h, [ref]$ef)) {
            Write-Host ("      extframe= ({0},{1})-({2},{3})  {4}x{5}" -f $ef.Left, $ef.Top, $ef.Right, $ef.Bottom, ($ef.Right - $ef.Left), ($ef.Bottom - $ef.Top))
        }
        if ($t -like '*OmniType_Preview*') { Write-Host "      ^^ PREVIEW" }
    }
}

$appPid = Get-AppPid
if (-not $appPid) { Write-Host "voice-ptt is not running."; exit 1 }
Write-Host "voice-ptt pid = $appPid"

if (-not $Watch) {
    Show-Windows $appPid
    exit 0
}

$end = (Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $end) {
    Write-Host ("--- {0:HH:mm:ss} ---" -f (Get-Date))
    Show-Windows $appPid
    Start-Sleep -Milliseconds $IntervalMs
}
