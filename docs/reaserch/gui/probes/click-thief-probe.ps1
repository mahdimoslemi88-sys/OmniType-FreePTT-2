<#
.SYNOPSIS
    Identifies which of our own top-level windows can still swallow mouse
    clicks, and which ones cannot.

.DESCRIPTION
    "Something invisible is eating my clicks" has exactly three possible
    culprits in a Win32 app, and a screenshot cannot tell them apart:

      1. a window with no click-through style and no window region — it takes
         the hit for its whole rect;
      2. a window whose region is smaller than its rect — it only takes the hit
         inside the region (this is what SetWindowRgn buys you);
      3. a window that is genuinely click-through — WS_EX_TRANSPARENT, or an
         empty region.

    This probe prints, for every visible top-level window of the voice-ptt
    process: its rect in physical pixels, its extended styles with
    WS_EX_TRANSPARENT decoded, its window region bounding box, and — the part
    that settles the question — what `WindowFromPoint` returns for a point in
    each corner and the centre of the window. A window that is still listed as
    owning its own corners is a click thief even when it looks fully
    transparent.

    It also reports the number of physical pixels the window covers on a
    1920x1080 screen, because a 238pt square window is invisible, harmless-looking,
    and still sits on top of whatever is under it.

.EXAMPLE
    powershell -File click-thief-probe.ps1
    powershell -File click-thief-probe.ps1 -Watch -Seconds 20
#>
[CmdletBinding()]
param(
    [switch]$Watch,
    [int]$Seconds = 10,
    [int]$IntervalMs = 500
)

$ErrorActionPreference = 'Stop'

# Without this the probe reports coordinates divided by the scale factor: a
# 238pt window on a 125% display measures as 190, and every area figure is off
# by 1.25x. This is the bug that invalidated the numbers in older sections of
# the artifact report, so it is fixed here rather than inherited.
try {
    Add-Type -Name DpiCtx -Namespace Probe -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
'@
    [Probe.DpiCtx]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
} catch {
    Write-Verbose "could not set per-monitor-v2 DPI awareness: $_"
}

Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public class ThiefProbe {
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
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("gdi32.dll")] public static extern IntPtr CreateRectRgn(int l, int t, int r, int b);
    [DllImport("gdi32.dll")] public static extern int GetRgnBox(IntPtr rgn, out RECT rect);
    [DllImport("gdi32.dll")] public static extern bool DeleteObject(IntPtr o);
    [DllImport("user32.dll")] public static extern int GetWindowRgn(IntPtr h, IntPtr rgn, int redraw);

    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X, Y; }

    public class Win {
        public IntPtr Hwnd;
        public string Title;
        public uint Pid;
        public int Left, Top, Right, Bottom;
        public long ExStyle;
        public uint Dpi;
        public bool HasRegion;
        public string RegionBox;
        public string CentreHit;
        public string CornerHits;
        public bool PassesClicks;
        public long AreaPx;
        public string Verdict;
    }

    public static IntPtr GetWindowLongPtr(IntPtr h, int i) {
        return IntPtr.Size == 8 ? GetWindowLongPtr64(h, i) : new IntPtr(GetWindowLong32(h, i));
    }

    const int GWL_EXSTYLE = -20;
    const long WS_EX_TRANSPARENT = 0x00000020;
    const long WS_EX_LAYERED    = 0x00080000;
    const long WS_EX_TOOLWINDOW = 0x00000080;
    const long WS_EX_NOACTIVATE = 0x08000000;

    static string TitleOf(IntPtr h) {
        var sb = new StringBuilder(512);
        GetWindowTextW(h, sb, sb.Capacity);
        return sb.ToString();
    }

    // Which window Windows actually hands the click to at this screen point.
    static string HitAt(int x, int y, IntPtr self) {
        IntPtr h = WindowFromPoint(new POINT { X = x, Y = y });
        if (h == IntPtr.Zero) return "none";
        if (h == self) return "SELF";
        return "other:" + TitleOf(h);
    }

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
            if (w <= 0 || ht <= 0) return true;

            long ex = GetWindowLongPtr(h, GWL_EXSTYLE).ToInt64();
            var win = new Win {
                Hwnd = h,
                Title = TitleOf(h),
                Pid = pid,
                Left = r.Left, Top = r.Top, Right = r.Right, Bottom = r.Bottom,
                ExStyle = ex,
                Dpi = GetDpiForWindow(h),
                AreaPx = (long)w * ht,
            };

            IntPtr probe = CreateRectRgn(0, 0, 0, 0);
            int err = GetWindowRgn(h, probe, 0);
            if (err == 0) {
                win.HasRegion = false;
                win.RegionBox = "(none: whole rect)";
            } else {
                RECT rb;
                if (GetRgnBox(probe, out rb) == 0) {
                    win.HasRegion = true;
                    win.RegionBox = "(empty: nothing)";
                } else {
                    win.HasRegion = true;
                    win.RegionBox = string.Format("{0},{1} {2}x{3}", rb.Left, rb.Top, rb.Right - rb.Left, rb.Bottom - rb.Top);
                }
            }
            DeleteObject(probe);

            int cx = (r.Left + r.Right) / 2, cy = (r.Top + r.Bottom) / 2;
            win.CentreHit = HitAt(cx, cy, h);
            win.CornerHits = string.Join("  ", new[] {
                HitAt(r.Left + 2, r.Top + 2, h),
                HitAt(r.Right - 3, r.Top + 2, h),
                HitAt(r.Left + 2, r.Bottom - 3, h),
                HitAt(r.Right - 3, r.Bottom - 3, h),
            });

            bool layered = (ex & WS_EX_LAYERED) != 0;
            bool transp  = (ex & WS_EX_TRANSPARENT) != 0;
            bool emptyRegion = win.HasRegion && win.RegionBox == "(empty: nothing)";
            win.PassesClicks = transp || emptyRegion;

            if (win.PassesClicks) {
                win.Verdict = "ok - click-through";
            } else if (win.CentreHit == "SELF") {
                win.Verdict = "THIEF - owns its centre";
            } else {
                win.Verdict = "ok - region excludes the centre";
            }
            if (layered && !win.PassesClicks) win.Verdict += " (layered)";

            result.Add(win);
            return true;
        }, IntPtr.Zero);
        return result;
    }
}
'@

function Invoke-Probe {
    $proc = Get-Process voice-ptt -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $proc) {
        Write-Host "voice-ptt is not running - nothing to probe" -ForegroundColor Yellow
        return $null
    }
    return [ThiefProbe]::ForProcess([uint32]$proc.Id)
}

function Format-Probe($wins) {
    if (-not $wins) { return }
    Write-Host ""
    Write-Host ("{0,-28} {1,-16} {2,5} {3,5} {4,5} {5,5}  {6,-12} {7,9}  {8}" -f `
        'WINDOW', 'RECT', 'W', 'H', 'DPI', 'ppp', 'REGION', 'AREApx', 'VERDICT')
    Write-Host ('-' * 118)
    foreach ($w in $wins) {
        $title = if ($w.Title) { $w.Title } else { '(untitled)' }
        if ($title.Length -gt 28) { $title = $title.Substring(0, 25) + '...' }
        $ppp = [math]::Round($w.Dpi / 96.0, 2)
        Write-Host ("{0,-28} {1,-16} {2,5} {3,5} {4,5} {5,5}  {6,-12} {7,9}  {8}" -f `
            $title, ("{0},{1}" -f $w.Left, $w.Top), ($w.Right - $w.Left), ($w.Bottom - $w.Top),
            $w.Dpi, $ppp, $w.RegionBox, $w.AreaPx, $w.Verdict)
        $flags = @()
        if (($w.ExStyle -band 0x00000020) -ne 0) { $flags += 'WS_EX_TRANSPARENT' }
        if (($w.ExStyle -band 0x00080000) -ne 0) { $flags += 'WS_EX_LAYERED' }
        if (($w.ExStyle -band 0x08000000) -ne 0) { $flags += 'WS_EX_NOACTIVATE' }
        if (($w.ExStyle -band 0x00000080) -ne 0) { $flags += 'WS_EX_TOOLWINDOW' }
        Write-Host ("    hwnd=0x{0:X}  centre={1}  corners=[{2}]  ex=[{3}]" -f `
            $w.Hwnd.ToInt64(), $w.CentreHit, $w.CornerHits, ($flags -join ','))
    }

    $thieves = @($wins | Where-Object { -not $_.PassesClicks })
    Write-Host ""
    if ($thieves.Count -eq 0) {
        Write-Host "Every window passes clicks through." -ForegroundColor Green
    } else {
        $total = ($thieves | Measure-Object -Property AreaPx -Sum).Sum
        Write-Host ("CLICK THIEVES: {0} window(s), {1:N0} px of always-on-top desktop they can swallow" -f `
            $thieves.Count, $total) -ForegroundColor Red
        foreach ($t in $thieves) {
            $t2 = if ($t.Title) { $t.Title } else { '(untitled)' }
            Write-Host ("  - {0}  {1}x{2}px at {3},{4}   centre={5}" -f `
                $t2, ($t.Right - $t.Left), ($t.Bottom - $t.Top), $t.Left, $t.Top, $t.CentreHit) -ForegroundColor Red
        }
    }
}

if ($Watch) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    do {
        Write-Host ("--- {0} ---" -f (Get-Date -Format 'HH:mm:ss'))
        Format-Probe (Invoke-Probe)
        Start-Sleep -Milliseconds $IntervalMs
    } while ((Get-Date) -lt $deadline)
} else {
    Format-Probe (Invoke-Probe)
}
