# The white halo: an automated hunt that is allowed to say "not reproduced".
#
# Two things this script refuses to do:
#
#   1. It will not report "no halo" unless halo-selftest.py passes in the SAME
#      run. A negative from an analyser nobody has seen fire is not evidence.
#   2. It will not analyse screenshots taken after a drag that failed to vacate
#      the old rect. That is the specific way this investigation produced a
#      confident wrong answer before: the leftover was the orb's own body, and
#      "bright pixels inside the old rect" was measuring the orb, not a halo.
#      The check is geometric, not a hope - see Verify-Vacated below.
#
# Why a drag at all: GUI-WINDOW-ARTIFACT-REPORT.md 16.2 identifies the trigger as
# *window movement*, not recording mode - the DWM keeps compositing the last
# frame that was shaped with the old region. Moving the window is therefore both
# the trigger and the way to empty the rect it left behind.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File halo-hunt.ps1
#   powershell -ExecutionPolicy Bypass -File halo-hunt.ps1 -KeepRunning
#
# Takes the mouse for a few seconds. Do not run it while doing anything else.

param(
    [string]$Exe = "",
    [string]$ShotDir = "",
    [switch]$KeepRunning
)

$ErrorActionPreference = 'Stop'
$ScriptDir = $PSScriptRoot

if (-not $Exe) {
    $guess = Join-Path $ScriptDir "..\..\..\..\voice-ptt\target\release\voice-ptt.exe"
    if (Test-Path $guess) { $Exe = (Resolve-Path $guess).Path }
    if (-not $Exe) { throw "voice-ptt.exe not found; pass -Exe explicitly." }
}
$Exe = (Resolve-Path $Exe).Path

Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public struct OrbRECT { public int Left, Top, Right, Bottom; }
public static class HaloWin {
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, ref OrbRECT r);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern IntPtr MonitorFromPoint(POINT p, uint f);
    [DllImport("user32.dll")] public static extern bool GetMonitorInfo(IntPtr m, ref MONITORINFO mi);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, IntPtr e);
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] public struct MONITORINFO {
        public int cbSize; public OrbRECT rcMonitor; public OrbRECT rcWork; public int dwFlags;
    }
    public static List<IntPtr> ForProcess(uint pid) {
        var list = new List<IntPtr>();
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == pid && IsWindowVisible(h)) list.Add(h);
            return true;
        }, IntPtr.Zero);
        return list;
    }
}
"@

# PER_MONITOR_AWARE_V2, and it must happen before the first window call.
#
# Without this line the probe lives in a different coordinate space from the
# screenshots it analyses: Windows PowerShell hosts are DPI-unaware by default,
# so GetWindowRect returns the logical screen divided by the scale factor while
# CopyFromScreen still captures physical pixels. Measured on this machine (scale
# 1.25): the app's own log says its window is 296x296 at centre (1813,257), and
# this probe, before the fix, reported 237x237 at (1450.5, 205.5) - which is
# 296/1.25 and 1813/1.25 exactly. A "the drag cleared the rect" check computed
# from numbers in the wrong space is a check of nothing.
[void][HaloWin]::SetProcessDpiAwarenessContext([IntPtr](-4))

$IDLE_PAINTED_REACH_PT = 60.89   # gui::orb::reach, measured in phase 4
$VACATE_MARGIN_PX = 8.0          # slack so a 1px sliver cannot pass the check

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

function Get-WorkArea {
    param([int]$X, [int]$Y)
    $mi = [HaloWin+MONITORINFO]::new()
    $mi.cbSize = [System.Runtime.InteropServices.Marshal]::SizeOf($mi)
    $p = [HaloWin+POINT]::new(); $p.X = $X; $p.Y = $Y
    [void][HaloWin]::GetMonitorInfo([HaloWin]::MonitorFromPoint($p, 2), [ref]$mi)
    return $mi.rcWork
}

function Invoke-OrbDrag {
    param([int]$FromX, [int]$FromY, [int]$ToX, [int]$ToY)
    [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point($FromX, $FromY)
    Start-Sleep -Milliseconds 250
    [HaloWin]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)   # LEFTDOWN
    Start-Sleep -Milliseconds 120
    $steps = 80
    for ($i = 1; $i -le $steps; $i++) {
        $x = [int]($FromX + ($ToX - $FromX) * $i / $steps)
        $y = [int]($FromY + ($ToY - $FromY) * $i / $steps)
        [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point($x, $y)
        Start-Sleep -Milliseconds 14
    }
    Start-Sleep -Milliseconds 200
    [HaloWin]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)   # LEFTUP
}

function Get-OrbWindow {
    param([int]$ProcId, $AtX, $AtY)
    # "First square window" is not the orb. A cloud-consent dialog is also
    # square, and EnumWindows order is not something to reason about — a run of
    # this script picked a 237x237 dialog instead of the 296x296 orb and
    # analysed the wrong window's rect. The app persists its own centre in
    # config.toml, so the orb is the square window whose centre matches it.
    $squares = @()
    foreach ($h in [HaloWin]::ForProcess([uint32]$ProcId)) {
        $r = [OrbRECT]::new()
        [void][HaloWin]::GetWindowRect($h, [ref]$r)
        $w = $r.Right - $r.Left; $ht = $r.Bottom - $r.Top
        if ([math]::Abs($w - $ht) -le 2 -and $w -ge 100) {
            $cx = ($r.Left + $r.Right) / 2; $cy = ($r.Top + $r.Bottom) / 2
            $sb = New-Object System.Text.StringBuilder 256
            [void][HaloWin]::GetClassName($h, $sb, 256)
            $dpi = [HaloWin]::GetDpiForWindow($h)
            $squares += [pscustomobject]@{
                H = $h; Rect = $r; Side = $w; CX = $cx; CY = $cy
                Class = $sb.ToString(); Dpi = $dpi
            }
        }
    }
    if ($squares.Count -eq 0) { return [IntPtr]::Zero }

    Write-Host "-- square windows in the process: $($squares.Count)"
    foreach ($s in $squares) {
        Write-Host ("     0x{0:X}  {1}x{1}px at ({2},{3}) centre=({4},{5})  dpi={6} class={7}" -f `
            $s.H.ToInt64(), $s.Side, $s.Rect.Left, $s.Rect.Top, $s.CX, $s.CY, $s.Dpi, $s.Class)
    }
    if ($AtX -ne $null -and $AtY -ne $null) {
        $best = $squares |
            Where-Object { [math]::Abs($_.CX - $AtX) -le 40 -and [math]::Abs($_.CY - $AtY) -le 40 } |
            Select-Object -First 1
        if ($best) {
            Write-Host ("-- matched by config position ({0},{1})" -f $AtX, $AtY)
            return $best.H
        }
        Write-Host "!! no square window matches the configured orb position"
        return [IntPtr]::Zero
    }
    # No config position available: the orb is the largest square, but say so,
    # because "largest" is a heuristic and the reader deserves to know.
    $largest = $squares | Sort-Object Side -Descending | Select-Object -First 1
    Write-Host ("-- NO config position; falling back to the largest square ({0}px)" -f $largest.Side)
    return $largest.H
}

# ---------------------------------------------------------------- the gate ---
Write-Host "== step 0: proving the analyser can fire, before trusting its silence =="
$selftest = & python (Join-Path $ScriptDir "halo-selftest.py") 2>&1
$selftest | ForEach-Object { Write-Host "   $_" }
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "VERDICT: INCONCLUSIVE - the analyser failed its own controls."
    Write-Host "         A halo hunt run with it would be guessing. Nothing was measured."
    exit 1
}

# ------------------------------------------------- config snapshot / restore ---
# Every drag rewrites orb_position_x/y. Leaving the orb somewhere else than the
# user left it is a side effect of a *diagnostic*, which is not acceptable.
function Get-ConfigPath {
    $beside = Join-Path (Split-Path $Exe -Parent) "config.toml"
    if (Test-Path $beside) { return $beside }
    return (Join-Path $env:APPDATA "voice-ptt\config.toml")
}
$config = Get-ConfigPath
$savedX = $null; $savedY = $null
$cfgX = $null; $cfgY = $null
if (Test-Path $config) {
    $text = Get-Content $config -Raw
    if ($text -match 'orb_position_x\s*=\s*(-?\d+)') { $savedX = $Matches[1]; $cfgX = [int]$savedX }
    if ($text -match 'orb_position_y\s*=\s*(-?\d+)') { $savedY = $Matches[1]; $cfgY = [int]$savedY }
    Write-Host "== config: $config (saved orb_position = $savedX,$savedY)"
} else {
    Write-Host "== config: $config (not found; nothing to restore)"
}

function Restore-OrbPosition {
    if ($savedX -eq $null -or $savedY -eq $null) { return }
    if (-not (Test-Path $config)) { return }
    $text = Get-Content $config -Raw
    $text = $text -replace 'orb_position_x\s*=\s*-?\d+', "orb_position_x = $savedX"
    $text = $text -replace 'orb_position_y\s*=\s*-?\d+', "orb_position_y = $savedY"
    # [System.IO.File]::WriteAllText, not Set-Content -Encoding UTF8: in Windows
    # PowerShell 5.1 the latter writes a BOM, and a BOM in front of a TOML file
    # is a corrupt first key. A diagnostic must not damage the file it inspects.
    [System.IO.File]::WriteAllText($config, $text)
    Write-Host "== restored orb_position = $savedX,$savedY in $config"
}

# Everything from here on moves the user's mouse and rewrites their config, so it
# runs inside try/finally: a crash between the drag and the end of the script
# must still put the orb back where they left it.
try {
    # --------------------------------------------------------- start the app ------
    Get-Process voice-ptt* -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 800

    $shotDir = if ($ShotDir) { $ShotDir } else { Join-Path $env:TEMP "halo-hunt-$(Get-Date -Format 'yyyyMMdd-HHmmss')" }
    New-Item -ItemType Directory -Force -Path $shotDir | Out-Null
    Write-Host "== screenshots: $shotDir"

    $proc = Start-Process -FilePath $Exe -PassThru
    $deadline = (Get-Date).AddSeconds(30)
    $h = [IntPtr]::Zero
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 400
        if ($proc.HasExited) { throw "voice-ptt exited immediately (code $($proc.ExitCode))." }
        $h = Get-OrbWindow -ProcId $proc.Id -AtX $cfgX -AtY $cfgY
        if ($h -ne [IntPtr]::Zero) { break }
    }
    if ($h -eq [IntPtr]::Zero) {
        $proc | Stop-Process -Force
        throw "no square orb window appeared within 30s"
    }
    Start-Sleep -Milliseconds 1500

    $old = [OrbRECT]::new(); [void][HaloWin]::GetWindowRect($h, [ref]$old)
    $ppp = [math]::Round(([HaloWin]::GetDpiForWindow($h)) / 96.0, 4)
    $side = $old.Right - $old.Left
    $ocx = [int](($old.Left + $old.Right) / 2); $ocy = [int](($old.Top + $old.Bottom) / 2)
    Write-Host ("== orb hwnd=0x{0:X} dpi={1} ppp={2}" -f $h.ToInt64(), ([HaloWin]::GetDpiForWindow($h)), $ppp)
    Write-Host ("== old rect: {0},{1} {2}x{2}px  centre=({3},{4})" -f $old.Left, $old.Top, $side, $ocx, $ocy)

    $work = Get-WorkArea -X $ocx -Y $ocy
    # The farthest work-area corner: a single long move, not four short edge nudges.
    $cands = @(
        @{ x = $work.Left + 40;   y = $work.Top + 40 },
        @{ x = $work.Right - 40;  y = $work.Top + 40 },
        @{ x = $work.Left + 40;   y = $work.Bottom - 40 },
        @{ x = $work.Right - 40;  y = $work.Bottom - 40 }
    )
    $best = $cands[0]; $bestD = -1
    foreach ($c in $cands) {
        $d = [math]::Sqrt(($c.x - $ocx) * ($c.x - $ocx) + ($c.y - $ocy) * ($c.y - $ocy))
        if ($d -gt $bestD) { $bestD = $d; $best = $c }
    }
    Write-Host ("== dragging once, {0}px, to the far corner ({1},{2})" -f [int]$bestD, $best.x, $best.y)

    $before = Save-Shot -Dir $shotDir -Name "1-before.png"
    Write-Host "-- before: $before"

    # A drag is an interaction, not a syscall: it can miss. So it is verified
    # and retried. A run that reports a halo after a drag that did not happen is
    # worse than a run that reports nothing.
    $attempts = 0
    $dragged = $false
    while (-not $dragged -and $attempts -lt 3) {
        $attempts++
        Write-Host "-- drag attempt $attempts"
        Invoke-OrbDrag -FromX $ocx -FromY $ocy -ToX $best.x -ToY $best.y
        Start-Sleep -Milliseconds 1500
        $chk = [OrbRECT]::new(); [void][HaloWin]::GetWindowRect($h, [ref]$chk)
        $dx = ($chk.Left + $chk.Right) / 2 - $ocx
        $dy = ($chk.Top + $chk.Bottom) / 2 - $ocy
        $d = [math]::Sqrt($dx * $dx + $dy * $dy)
        Write-Host ("   the orb moved {0}px" -f [int]$d)
        if ($d -gt 20) { $dragged = $true }
        else { Start-Sleep -Milliseconds 2500 }   # let the app finish booting
    }

    $new = [OrbRECT]::new(); [void][HaloWin]::GetWindowRect($h, [ref]$new)
    $ncx = [int](($new.Left + $new.Right) / 2); $ncy = [int](($new.Top + $new.Bottom) / 2)
    $after = Save-Shot -Dir $shotDir -Name "2-after.png"
    Write-Host "-- after: $after"
    Write-Host ("== orb moved to ({0},{1}); new rect {2},{3}" -f $ncx, $ncy, $new.Left, $new.Top)

    # --------------------------------------------------- the geometry check ------
    # The old rect must contain no part of the orb's new painted disc. The worst
    # point of a square of side S is its corner, at S*sqrt(2)/2 from the centre, so
    # the requirement is a straight-line distance, not a per-axis one.
    $paintR = $IDLE_PAINTED_REACH_PT * $ppp
    $needed = ($side * [math]::Sqrt(2) / 2) + $paintR + $VACATE_MARGIN_PX
    $moved = [math]::Sqrt(($ncx - $ocx) * ($ncx - $ocx) + ($ncy - $ocy) * ($ncy - $ocy))
    Write-Host ("== vacate check: moved {0}px, needs > {1}px (rect corner {2} + orb {3} + margin {4})" -f `
        [int]$moved, [int]$needed, [int]($side * [math]::Sqrt(2) / 2), [int]$paintR, [int]$VACATE_MARGIN_PX)

    # ---------------------------------------------------------------- analyse -----
    $analyse = Join-Path $ScriptDir "arc-analyse.py"
    $out = & python $analyse $before $after $old.Left $old.Top $side 2>&1
    $out | ForEach-Object { Write-Host "   $_" }
    $token = ($out | Where-Object { $_ -match '^VERDICT:' } | Select-Object -Last 1)
    $token = if ($token) { ($token -split ':', 2)[1].Trim() } else { "NO-VERDICT" }

    # ------------------------------------------------------------- restore -------


} finally {
    Restore-OrbPosition
    if (-not $KeepRunning -and $proc -and -not $proc.HasExited) {
        $proc | Stop-Process -Force
    }
}if ($moved -le $needed) {
        Write-Host "VERDICT: INCONCLUSIVE - the drag did not clear the old rect."
        Write-Host "         The orb is still inside it, so anything bright there is the orb."
        Write-Host "         This is the exact trap that made the previous runs worthless."
        Write-Host "         (the analyser said: $token - ignored, because the geometry is not sound)"
        exit 1
    }
    if ($token -eq "REPRODUCED") {
        Write-Host "VERDICT: REPRODUCED - a thin bright arc is left behind in the rect the orb vacated."
        exit 1
    }
Write-Host "VERDICT: NOT REPRODUCED"
Write-Host "         The analyser fired on its own controls in this same run, and the"
Write-Host "         orb moved $needed px clear of the rect it left. A negative here is"
Write-Host "         evidence, not silence."
exit 0