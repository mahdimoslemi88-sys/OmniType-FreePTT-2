<#
.SYNOPSIS
  OmniType FreePTT window *style* probe (caption / border / popup / layered).

.DESCRIPTION
  `window-probe.ps1` reports geometry (where each window is). This one reports
  the style bits, which is what makes a window look "framed": after the phase 1
  regression the orb window came back with WS_CAPTION | WS_THICKFRAME |
  WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX (style 0x16CB0000) — a real
  Windows title bar with minimize/maximize/close around the orb — and the
  transcript card showed the system background because its preview window was
  unshaped too. The fix (phase 3.2, `enforce_frameless_window`) checks these bits
  every frame and re-strips them the moment they drift back.

  Expected output for the app's own windows:

    class=Window Class   caption=False popup=True     <- orb: frameless
    class=OmniType_Preview caption=False popup=True   <- transcript card
    class=Winit Thread Event Target / tray_icon_app   <- winit's own windows;
                                                         their bits are not ours

  Read-only: EnumWindows + GetWindow* only.

.EXAMPLE
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File window-style-probe.ps1
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File window-style-probe.ps1 -TargetPid 21576
#>
param(
    [int]$TargetPid = 0
)

Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;

public static class OmniStyleProbe
{
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern int GetWindowLongW(IntPtr hWnd, int index);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    /// PowerShell is not per-monitor DPI aware, so GetWindowRect is virtualised
    /// (203x203 physical px reads as 162x162 at 125% scaling). Call before Dump().
    public static string MakeDpiAware()
    {
        // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
        if (SetProcessDpiAwarenessContext(new IntPtr(-4)))
            return "per-monitor-v2 (rects below are physical pixels)";
        if (SetProcessDPIAware())
            return "system (rects below are physical pixels)";
        return "FAILED - rects below are virtualised (multiply by the scale factor)";
    }

    public static System.Collections.Generic.List<string> Dump(uint target)
    {
        var lines = new System.Collections.Generic.List<string>();
        EnumWindows(delegate(IntPtr h, IntPtr l)
        {
            uint pid;
            GetWindowThreadProcessId(h, out pid);
            if (pid != target) return true;

            var cls = new StringBuilder(256);
            GetClassNameW(h, cls, cls.Capacity);
            var title = new StringBuilder(256);
            GetWindowTextW(h, title, title.Capacity);
            RECT r;
            GetWindowRect(h, out r);
            int style = GetWindowLongW(h, -16);   // GWL_STYLE
            int ex = GetWindowLongW(h, -20);      // GWL_EXSTYLE
            bool caption = (style & 0x00C00000) != 0;   // WS_CAPTION
            bool thickframe = (style & 0x00040000) != 0; // WS_THICKFRAME
            bool popup = (style & unchecked((int)0x80000000)) != 0; // WS_POPUP
            bool edged = (ex & 0x00000100) != 0 || (ex & 0x00000200) != 0; // WINDOWEDGE | CLIENTEDGE
            bool layered = (ex & 0x00080000) != 0;

            lines.Add(string.Format(
                "class={0,-26} vis={1,-5} min={2,-5} dpi={3,-4} rect=({4},{5} {6}x{7}) style=0x{8:X8} ex=0x{9:X8} caption={10,-5} thickframe={11,-5} popup={12,-5} edged={13,-5} layered={14,-5} title=\"{15}\"",
                cls.ToString(), IsWindowVisible(h), IsIconic(h), GetDpiForWindow(h),
                r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top,
                style, ex, caption, thickframe, popup, edged, layered, title.ToString()));
            return true;
        }, IntPtr.Zero);
        lines.Sort();
        return lines;
    }
}
"@

$proc = if ($TargetPid -gt 0) { Get-Process -Id $TargetPid -ErrorAction SilentlyContinue }
        else { Get-Process -Name voice-ptt -ErrorAction SilentlyContinue | Select-Object -First 1 }

if (-not $proc) {
    Write-Output "voice-ptt.exe is not running."
    exit 1
}

Write-Output ("-- dpi: {0}" -f [OmniStyleProbe]::MakeDpiAware())
Write-Output ("-- pid={0}  private={1} MB  working_set={2} MB  {3}" -f `
    $proc.Id, `
    [math]::Round($proc.PrivateMemorySize64 / 1MB, 1), `
    [math]::Round($proc.WorkingSet64 / 1MB, 1), `
    (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Write-Output "-- private must stay ~300 MB: ~1.9 GB means the 1.6 GB local whisper model got loaded"
Write-Output ""
[OmniStyleProbe]::Dump([uint32]$proc.Id) | ForEach-Object { Write-Output $_ }
