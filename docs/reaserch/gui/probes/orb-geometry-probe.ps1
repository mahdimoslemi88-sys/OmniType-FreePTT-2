<#
.SYNOPSIS
    Measures the live orb window: its rect, its click region, and whether either
    of them moves while the orb is idle.

.DESCRIPTION
    Three questions this answers that a unit test cannot:

      1. How big is the host window in *physical pixels* on this display, and is
         it the size the geometry derivation predicts?
      2. Is the window really fixed? Every phase of this app's history that
         produced a visible artifact involved a transparent always-on-top window
         being moved or resized. A rect that is constant across the idle, hover
         and mode-change samples is the evidence that the "created once, never
         resized" claim still holds.
      3. Is the click region a circle of the painted reach rather than the whole
         rect? That is what lets clicks reach the application underneath the
         transparent part of the window.

    Run it on its own: it launches the app, and the drag/hover samples want the
    mouse to itself.

.EXAMPLE
    powershell -File orb-geometry-probe.ps1
    powershell -File orb-geometry-probe.ps1 -Seconds 12 -Hover
#>
[CmdletBinding()]
param(
    [string]$Exe = "",
    [int]$Seconds = 8,
    [int]$IntervalMs = 400,
    # Sweep the mouse across the orb to force a hover sample.
    [switch]$Hover,
    # Synthesise a drag against each work-area edge and measure the result.
    [switch]$Drag,
    [string]$ShotDir = ""
)

$ErrorActionPreference = 'Stop'

if (-not $Exe) {
    $guess = Join-Path $PSScriptRoot "..\..\..\..\voice-ptt\target\release\voice-ptt.exe"
    $Exe = (Resolve-Path $guess -ErrorAction SilentlyContinue)
    if (-not $Exe) { throw "voice-ptt.exe not found; pass -Exe explicitly." }
}
$Exe = (Resolve-Path $Exe).Path

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class OrbWin {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    // GetWindowRgn: returns the region type, 0 = error, 1 = NULLREGION, 2 = SIMPLEREGION, 3 = COMPLEXREGION
    [DllImport("user32.dll")] public static extern int GetWindowRgn(IntPtr h, IntPtr rgn);
    [DllImport("gdi32.dll")] public static extern int GetRgnBox(IntPtr rgn, out RECT box);
    [DllImport("gdi32.dll")] public static extern IntPtr CreateRectRgn(int l, int t, int r, int b);
    [DllImport("gdi32.dll")] public static extern bool DeleteObject(IntPtr o);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] public static extern IntPtr MonitorFromPoint(POINT p, uint f);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(int x, int y);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", EntryPoint="GetWindowLongW")] public static extern int GetWindowLongW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern IntPtr GetTopWindow(IntPtr h, uint flags);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [StructLayout(LayoutKind.Sequential)]
    public struct MONITORINFO { public int cbSize; public RECT rcMonitor; public RECT rcWork; public int dwFlags; }
    [DllImport("user32.dll")] public static extern bool GetMonitorInfo(IntPtr m, ref MONITORINFO i);
}
'@

# Per-monitor v2, so GetWindowRect is physical pixels and the numbers line up
# with what the app computed.
[void][OrbWin]::SetProcessDpiAwarenessContext([IntPtr](-4))

function Save-Shot {
    param([string]$Dir, [string]$Name)
    New-Item -ItemType Directory -Force -Path $Dir | Out-Null
    $path = Join-Path $Dir $Name
    $bmp = New-Object System.Drawing.Bitmap 1920, 1080
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen(0, 0, 0, 0, (New-Object System.Drawing.Size(1920, 1080)))
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    return $path
}

$MONITOR_DEFAULTTONEAREST = 2

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class OrbMouse {
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, IntPtr e);
    public static void Left(uint flag) { mouse_event(flag, 0, 0, 0, IntPtr.Zero); }
}
'@

Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class EnumWin {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", EntryPoint="GetWindowLongW")] public static extern int GetWindowLongW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern IntPtr GetTopWindow(IntPtr h, uint flags);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, System.Text.StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr h);
    public static List<IntPtr> ForProcess(uint want) {
        var list = new List<IntPtr>();
        EnumWindows((h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (pid == want && IsWindowVisible(h)) list.Add(h);
            return true;
        }, IntPtr.Zero);
        return list;
    }
}
'@

function Get-MonitorWorkArea {
    param([int]$X, [int]$Y)
    $p = [OrbWin+POINT]::new(); $p.X = $X; $p.Y = $Y
    $m = [OrbWin]::MonitorFromPoint($p, $MONITOR_DEFAULTTONEAREST)
    $mi = [OrbWin+MONITORINFO]::new()
    $mi.cbSize = [System.Runtime.InteropServices.Marshal]::SizeOf($mi)
    [void][OrbWin]::GetMonitorInfo($m, [ref]$mi)
    $mi.rcWork
}

Write-Host "== orb geometry probe"
Write-Host "-- exe: $Exe"

# The orb window is the square one: the orb is drawn centred in a fixed square
# canvas. winit also creates an 18x18 "Thread Event Target" window, so waiting
# for "any window" is not enough - that one appears first and every time.
function Get-OrbCandidates {
    param([int]$ProcId)
    $out = @()
    foreach ($cand in [EnumWin]::ForProcess([uint32]$ProcId)) {
        $cr = [OrbWin+RECT]::new()
        [void][OrbWin]::GetWindowRect($cand, [ref]$cr)
        $cw = $cr.Right - $cr.Left
        $ch = $cr.Bottom - $cr.Top
        if ([math]::Abs($cw - $ch) -le 2 -and $cw -ge 100) { $out += $cand }
    }
    return $out
}

$proc = Start-Process -FilePath $Exe -PassThru
$deadline = (Get-Date).AddSeconds(25)
$candidates = @()
while ((Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 400
    if ($proc.HasExited) { throw "voice-ptt exited immediately (code $($proc.ExitCode))." }
    $candidates = Get-OrbCandidates -ProcId $proc.Id
    if ($candidates.Count -gt 0) { break }
}
if ($candidates.Count -eq 0) {
    $proc | Stop-Process -Force
    throw "no square orb window appeared within 25s"
}
$handles = [EnumWin]::ForProcess([uint32]$proc.Id)

Write-Host "-- visible top-level windows owned by the process:"
foreach ($cand in $handles) {
    $cr = [OrbWin+RECT]::new()
    [void][OrbWin]::GetWindowRect($cand, [ref]$cr)
    $cw = $cr.Right - $cr.Left; $ch = $cr.Bottom - $cr.Top
    $cdpi = [OrbWin]::GetDpiForWindow($cand)
    $sb = New-Object System.Text.StringBuilder 256
    [void][EnumWin]::GetClassName($cand, $sb, 256)
    Write-Host ("   0x{0:X}  {1}x{2} px  dpi={3}  class={4}" -f $cand.ToInt64(), $cw, $ch, $cdpi, $sb.ToString())
    # The orb window is the square one: the orb is drawn centred in a fixed
    # square canvas, so a non-square top-level window is something else.
}
if ($candidates.Count -gt 1) { Write-Host "   (several square windows; using the first)" }
$h = $candidates[0]
$r = [OrbWin+RECT]::new()
[void][OrbWin]::GetWindowRect($h, [ref]$r)
$dpi = [OrbWin]::GetDpiForWindow($h)
$ppp = [math]::Round($dpi / 96.0, 4)
$side_px = $r.Right - $r.Left
$side_pt = [math]::Round($side_px / $ppp, 2)
Write-Host ("-- hwnd=0x{0:X}  dpi={1}  pixels_per_point={2}" -f $h.ToInt64(), $dpi, $ppp)
Write-Host ("-- window rect: {0}x{1} px at ({2},{3})  =  {4} pt square" -f `
    $side_px, ($r.Bottom - $r.Top), $r.Left, $r.Top, $side_pt)

$work = Get-MonitorWorkArea -X (($r.Left + $r.Right) / 2) -Y (($r.Top + $r.Bottom) / 2)
Write-Host ("-- monitor work area: {0},{1} .. {2},{3}  (taskbar excluded)" -f `
    $work.Left, $work.Top, $work.Right, $work.Bottom)
$gapL = (($r.Left + $r.Right) / 2) - $work.Left
$gapR = $work.Right - (($r.Left + $r.Right) / 2)
Write-Host ("-- orb centre distance to work-area edge: left={0}px right={1}px" -f $gapL, $gapR)

# ── the click region ───────────────────────────────────────────────────
$rgn = [OrbWin]::CreateRectRgn(0, 0, 0, 0)
$kind = [OrbWin]::GetWindowRgn($h, $rgn)
$box = [OrbWin+RECT]::new()
[void][OrbWin]::GetRgnBox($rgn, [ref]$box)
[void][OrbWin]::DeleteObject($rgn)
$kindName = switch ($kind) { 0 { "ERROR" } 1 { "NULLREGION (whole window)" } 2 { "SIMPLEREGION" } 3 { "COMPLEXREGION" } default { "type $kind" } }
Write-Host ("-- click region: {0}  bbox={1}x{2} px" -f `
    $kindName, ($box.Right - $box.Left), ($box.Bottom - $box.Top))
if ($kind -eq 0) { Write-Host "   !! GetWindowRgn failed" }
if ($kind -eq 1) { Write-Host "   !! the window owns its whole rect: it would steal every click" }

# ── is the window actually fixed? ──────────────────────────────────────
Write-Host ""
Write-Host "-- click-through: which window owns a point?"
$rr = [OrbWin+RECT]::new()
[void][OrbWin]::GetWindowRect($h, [ref]$rr)
$probes = @(
    @{ name = "orb centre (must be the orb)";      x = [int](($rr.Left + $rr.Right) / 2); y = [int](($rr.Top + $rr.Bottom) / 2) },
    @{ name = "window corner, 6px in (transparent)"; x = $rr.Left + 6;  y = $rr.Top + 6 },
    @{ name = "window corner, 20px in (transparent)"; x = $rr.Left + 20; y = $rr.Top + 20 }
)
$vis = [OrbWin]::IsWindowVisible($h)
$ex = [OrbWin]::GetWindowLongW($h, -20)   # GWL_EXSTYLE
$topmost = (($ex -band 0x00000008) -ne 0)  # WS_EX_TOPMOST
Write-Host ("   orb window WS_EX_TOPMOST={0} (exstyle=0x{1:X8}, WS_EX_LAYERED={2})" -f `
    $topmost, $ex, (($ex -band 0x00080000) -ne 0))
# WS_EX_TOPMOST only means "above the non-topmost band"; another topmost window
# can still be in front. Walk the topmost band and count what is above the orb.
$above = 0
$w = [OrbWin]::GetTopWindow([IntPtr]::Zero, 0)   # GW_HWNDFIRST
$seen = 0
while ($w -ne [IntPtr]::Zero -and $seen -lt 200) {
    if ($w -eq $h) { break }
    if ([OrbWin]::IsWindowVisible($w)) { $above++ }
    $w = [OrbWin]::GetTopWindow($w, 1)           # GW_HWNDNEXT
    $seen++
}
Write-Host ("   visible windows in the top z-order band above the orb: {0}" -f $above)
Write-Host ("   (orb window visible={0}; a probe point landing over another app reports" -f $vis)
Write-Host "    that app instead, which says nothing about the region)"
foreach ($pt in $probes) {
    $owner = [OrbWin]::WindowFromPoint($pt.x, $pt.y)
    if ($owner -eq [IntPtr]::Zero) {
        Write-Host ("   {0,-42} -> desktop" -f $pt.name)
        continue
    }
    $isOrb = ($owner -eq $h)
    $sb = New-Object System.Text.StringBuilder 256
    [void][EnumWin]::GetClassName($owner, $sb, 256)
    $label = if ($isOrb) { "the ORB window" } else { "something else (class $($sb.ToString())) - click passes through" }
    Write-Host ("   {0,-42} -> {1}" -f $pt.name, $label)
}
Write-Host ""
Write-Host "-- sampling the rect every ${IntervalMs}ms for ${Seconds}s (idle)"
$samples = @()
for ($i = 0; $i -lt [math]::Floor($Seconds * 1000 / $IntervalMs); $i++) {
    $rr = [OrbWin+RECT]::new()
    [void][OrbWin]::GetWindowRect($h, [ref]$rr)
    $samples += ("{0}x{1}@{2},{3}" -f ($rr.Right - $rr.Left), ($rr.Bottom - $rr.Top), $rr.Left, $rr.Top)
    if ($Hover -and ($i % 4 -eq 2)) {
        $cx = [int](($rr.Left + $rr.Right) / 2); $cy = [int](($rr.Top + $rr.Bottom) / 2)
        [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point($cx, $cy)
    }
    Start-Sleep -Milliseconds $IntervalMs
}
if ($Hover) { [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point(10, 10) }

$distinct = $samples | Sort-Object -Unique
Write-Host ("-- {0} samples, {1} distinct rect(s)" -f $samples.Count, $distinct.Count)
$distinct | ForEach-Object { Write-Host "   $_" }
if ($distinct.Count -eq 1) {
    Write-Host "   OK: the window never moved or resized while idle"
} else {
    Write-Host "   !! the window is not fixed; every one of these is a SetWindowPos"
}

# ── drag to the edges ──────────────────────────────────────────────────
# The point of the new clamp is that the orb can sit *close* to the edge while
# its painted pixels stay on screen. That is measurable without a human: a
# synthetic press-move-release, then the distance from the orb's centre to the
# work-area edge compared against the painted reach.
function Invoke-OrbDrag {
    param([IntPtr]$Hwnd, [int]$ToX, [int]$ToY)
    $rr = [OrbWin+RECT]::new()
    [void][OrbWin]::GetWindowRect($Hwnd, [ref]$rr)
    $cx = [int](($rr.Left + $rr.Right) / 2)
    $cy = [int](($rr.Top + $rr.Bottom) / 2)
    [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point($cx, $cy)
    Start-Sleep -Milliseconds 250
    # press
    [OrbMouse]::Left(0x0002)      # MOUSEEVENTF_LEFTDOWN
    Start-Sleep -Milliseconds 120
    # move in steps: a single jump is not a drag as far as egui is concerned
    $steps = 48
    for ($i = 1; $i -le $steps; $i++) {
        $x = [int]($cx + ($ToX - $cx) * $i / $steps)
        $y = [int]($cy + ($ToY - $cy) * $i / $steps)
        [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point($x, $y)
        Start-Sleep -Milliseconds 18
    }
    Start-Sleep -Milliseconds 150
    [OrbMouse]::Left(0x0004)      # MOUSEEVENTF_LEFTUP
    Start-Sleep -Milliseconds 400
}

# One screenshot per stage. The pair is what makes the halo question decidable:
# anything bright inside the rect the orb *used* to own, and absent from the
# earlier shot, is a leftover rather than part of the desktop.
if ($ShotDir) {
    Write-Host ""
    Write-Host ("-- screenshot before any drag: " + (Save-Shot -Dir $ShotDir -Name "1-before.png"))
    if ($Drag) {
        Write-Host "-- (now dragging; the next screenshot is taken afterwards)"
    }
}

if ($Drag) {
    Write-Host ""
    Write-Host "-- dragging the orb against each work-area edge"
    $rr = [OrbWin+RECT]::new()
    [void][OrbWin]::GetWindowRect($h, [ref]$rr)
    $cx = [int](($rr.Left + $rr.Right) / 2)
    $cy = [int](($rr.Top + $rr.Bottom) / 2)
    $targets = @(
        @{ name = "right";  x = $work.Right - 4;  y = $cy },
        @{ name = "bottom"; x = $cx;               y = $work.Bottom - 4 },
        @{ name = "left";   x = $work.Left + 4;   y = $cy },
        @{ name = "top";    x = $cx;               y = $work.Top + 4 }
    )
    foreach ($t in $targets) {
        Invoke-OrbDrag -Hwnd $h -ToX $t.x -ToY $t.y
        # The orb is still springing towards its resting scale for a moment after
        # the drag, and the clamp uses the *current* scale, so read it twice: the
        # second read is the one that shows where it settles.
        Start-Sleep -Milliseconds 1200
        $ar = [OrbWin+RECT]::new()
        [void][OrbWin]::GetWindowRect($h, [ref]$ar)
        $ax = ($ar.Left + $ar.Right) / 2
        $ay = ($ar.Top + $ar.Bottom) / 2
        $gapPx = [math]::Min(
            [math]::Min($ax - $work.Left, $work.Right - $ax),
            [math]::Min($ay - $work.Top, $work.Bottom - $ay))
        $gapPt = [math]::Round($gapPx / $ppp, 2)
        # The idle painted reach in points, at 125%: 60.89 pt. The orb cannot be
        # closer to the edge than its own glow, so a gap well under that means
        # pixels are being cut.
        $idleReachPt = 60.89
        $verdict = if ($gapPt -lt ($idleReachPt * 0.92)) {
            "!! closer than the orb's own glow: pixels are off-screen"
        } elseif ($gapPt -lt 80) { "close to the edge (expected)" } else { "still held back" }
        Write-Host ("   {0,-7} centre=({1},{2})  nearest edge gap={3} px = {4} pt   {5}" -f `
            $t.name, [int]$ax, [int]$ay, [int]$gapPx, $gapPt, $verdict)
    }
    [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point(10, 10)
}

# ── click-through ──────────────────────────────────────────────────────
# The region is a circle of the painted reach, so a point in the transparent
# corner of the window has to belong to whatever is underneath.
# `WindowFromPoint` answers exactly that, and it is the same question a user
# answers by clicking.
#
# Read this before trusting a negative result: `WS_EX_TOPMOST` only means "above
# the non-topmost band", and a probe point that lands over another application's
# window reports that application no matter what the orb's region is. The probe
# therefore prints the z-order position and the ex-style alongside the answer.
$vis = [OrbWin]::IsWindowVisible($h)
$ex = [OrbWin]::GetWindowLongW($h, -20)   # GWL_EXSTYLE
$topmost = (($ex -band 0x00000008) -ne 0)  # WS_EX_TOPMOST
$layered = (($ex -band 0x00080000) -ne 0)
Write-Host ("   orb window visible={0}  WS_EX_TOPMOST={1}  WS_EX_LAYERED={2}  (exstyle=0x{3:X8})" -f `
    $vis, $topmost, $layered, $ex)
$above = 0
$w = [OrbWin]::GetTopWindow([IntPtr]::Zero, 0)   # GW_HWNDFIRST
$seen = 0
while ($w -ne [IntPtr]::Zero -and $seen -lt 200) {
    if ($w -eq $h) { break }
    if ([OrbWin]::IsWindowVisible($w)) { $above++ }
    $w = [OrbWin]::GetTopWindow($w, 1)           # GW_HWNDNEXT
    $seen++
}
Write-Host ("   visible windows in the z-order band above the orb: {0}" -f $above)
if ($above -gt 0) {
    Write-Host "   !! something is in front of the orb: every answer below is about"
    Write-Host "      that window's own hit-testing, not about the orb region"
}

$rr = [OrbWin+RECT]::new()
[void][OrbWin]::GetWindowRect($h, [ref]$rr)
$probes = @(
    @{ name = "orb centre (must be the orb)";          x = [int](($rr.Left + $rr.Right) / 2); y = [int](($rr.Top + $rr.Bottom) / 2) },
    @{ name = "window corner, 6px in (transparent)";  x = $rr.Left + 6;  y = $rr.Top + 6 },
    @{ name = "window corner, 20px in (transparent)"; x = $rr.Left + 20; y = $rr.Top + 20 }
)
foreach ($pt in $probes) {
    $owner = [OrbWin]::WindowFromPoint($pt.x, $pt.y)
    if ($owner -eq [IntPtr]::Zero) {
        Write-Host ("   {0,-42} -> desktop" -f $pt.name)
        continue
    }
    $sb = New-Object System.Text.StringBuilder 256
    [void][EnumWin]::GetClassName($owner, $sb, 256)
    $label = if ($owner -eq $h) { "the ORB window" } else { "class $($sb.ToString()) - not the orb" }
    Write-Host ("   {0,-42} -> {1}" -f $pt.name, $label)
}

if ($ShotDir) {
    Write-Host ("-- screenshot after:           " + (Save-Shot -Dir $ShotDir -Name "2-after.png"))
    Write-Host "   Compare with 1-before.png using arc-analyse.py: pass the OLD window"
    Write-Host "   rect and side from the header line above, not the new one."
}

Write-Host ""
Write-Host "-- stopping the app"
$proc | Stop-Process -Force
Start-Sleep -Milliseconds 400
Write-Host "done"
