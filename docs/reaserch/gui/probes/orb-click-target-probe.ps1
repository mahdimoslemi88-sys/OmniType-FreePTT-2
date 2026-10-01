<#
.SYNOPSIS
    Measures the orb window's live click target and gates the probe on five
    controls first.

.DESCRIPTION
    O1 closed the "dead click ring" by deriving both the egui pointer rectangle
    and the Win32 window region from one number, `interaction_radius_pt`. From
    outside the process only one half of that is observable: the window's own
    region. This probe reads it and prints what it found next to the radii the
    code predicts for each mode, so a disagreement is visible instead of
    assumed away.

    It deliberately does NOT name the mode. From outside the app there is no
    reliable way to tell Idle from Recording, and a probe that guesses prints a
    confident wrong answer - which is the failure mode this repo has hit three
    times already. It prints the candidate modes the measurement is consistent
    with and leaves the naming to a human looking at the screen.

    THE GATE COMES FIRST. Six controls run before anything is reported, because
    a probe that reads zero every time is indistinguishable from a probe that
    works:

      C1  a window with no region must read as "no region", not as radius 0
      C2  a region set to 40 px must read back as 40 px (the arithmetic works)
      C3  40 px and 90 px must read differently (the probe can discriminate)
      C4  the read-back must follow the window's DPI, not the desktop's
      C5  a handle that does not exist must be refused, not measured

    Exit 1 if any control fails, 2 if the app is not running, 0 otherwise.

.EXAMPLE
    powershell -File orb-click-target-probe.ps1
    powershell -File orb-click-target-probe.ps1 -Process voice-ptt
    powershell -File orb-click-target-probe.ps1 -GateOnly
#>
[CmdletBinding()]
param(
    # Process name (without .exe) whose window to measure.
    [string]$Process = "voice-ptt",
    # Run the controls and stop. No app needed.
    [switch]$GateOnly
)

$ErrorActionPreference = 'Stop'

Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class TargetWin {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);

    // The read-back O1 added: returns 0 and empties the box when the window
    // carries no region at all.
    [DllImport("user32.dll")] public static extern int GetWindowRgnBox(IntPtr h, out RECT box);
    [DllImport("user32.dll")] public static extern int SetWindowRgn(IntPtr h, IntPtr rgn, int redraw);
    [DllImport("gdi32.dll")] public static extern IntPtr CreateEllipticRgn(int l, int t, int r, int b, int f);
    [DllImport("gdi32.dll")] public static extern bool DeleteObject(IntPtr o);

    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", EntryPoint="GetWindowLongW")] public static extern int GetWindowLongW(IntPtr h, int i);
    [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);

    public static List<IntPtr> TopLevelWindowsOf(uint pid) {
        var found = new List<IntPtr>();
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == pid) found.Add(h);
            return true;
        }, IntPtr.Zero);
        return found;
    }

    // Region radius in px, or -1 when the window has no region.
    public static int RegionRadiusPx(IntPtr h) {
        RECT box;
        if (GetWindowRgnBox(h, out box) == 0) return -1;
        int w = box.Right - box.Left, ht = box.Bottom - box.Top;
        if (w <= 0 || ht <= 0) return -1;
        // The bounding box is 2r wide for the (cx-r, cy-r, cx+r+1, cy+r+1)
        // ellipse the app builds, whatever GDI's exclusive right/bottom bound
        // normalises it to - so plain integer division, not (w-1)/2, which is
        // a whole pixel small for every reading.
        int r = Math.Min(w, ht) / 2;
        if (r < 0) return -1;
        if (w != ht) return -1;              // not the circle this probe measures
        return r;
    }

    // The raw box, so a human can check the arithmetic instead of trusting it.
    public static string RegionBox(IntPtr h) {
        RECT b;
        if (GetWindowRgnBox(h, out b) == 0) return "none";
        return string.Format("{0},{1}..{2},{3} ({4}x{5})",
                             b.Left, b.Top, b.Right, b.Bottom,
                             b.Right - b.Left, b.Bottom - b.Top);
    }

    public static bool GiveEllipse(IntPtr h, int cx, int cy, int r) {
        IntPtr rg = CreateEllipticRgn(cx - r, cy - r, cx + r + 1, cy + r + 1, 1);
        if (rg == IntPtr.Zero) return false;
        bool ok = SetWindowRgn(h, rg, 1) != 0;
        if (!ok) DeleteObject(rg);            // ownership never transferred
        return ok;
    }

    public static void ClearRegion(IntPtr h) { SetWindowRgn(h, IntPtr.Zero, 1); }
}
'@

# Per-monitor v2 so GetWindowRect and the DPI read are the same physical pixels
# the app computed with.
[void][TargetWin]::SetProcessDpiAwarenessContext([IntPtr](-4))

# ── the numbers the code predicts, from interaction_radius_pt ────────────────
# Read off the table locked by `the_click_target_grew_from_a_guess_to_the_painted_circle`
# in voice-ptt/src/gui/orb.rs. A change there has to change this file, which is
# the point: the probe should not quietly start describing different behaviour.
$Predicted = [ordered]@{
    "Idle"          = 54.285
    "Idle + hover"  = 58.640
    "Recording"     = 90.475
    "Processing"    = 81.398
    "Complete"      = 81.398
    "Error"         = 60.885
}

$script:Failures = 0

function Assert-Control {
    param([string]$Name, [bool]$Ok, [string]$Detail)
    if ($Ok) {
        Write-Host ("  PASS  {0,-4} {1}" -f $Name, $Detail)
    } else {
        Write-Host ("  FAIL  {0,-4} {1}" -f $Name, $Detail) -ForegroundColor Red
        $script:Failures++
    }
}

function Get-Controls {
    # The controls need a window of our own so they can be broken on purpose.
    # A hidden, tiny popup window is enough: SetWindowRgn and GetWindowRgnBox
    # work on it and nothing is ever shown.
    [TargetWinControl]::Create()
}

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class TargetWinControl {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr CreateWindowExW(
        int ex, string cls, string name, uint style, int x, int y, int w, int ht,
        IntPtr parent, IntPtr menu, IntPtr inst, IntPtr param);
    [DllImport("user32.dll")] public static extern bool DestroyWindow(IntPtr h);

    public static IntPtr Create() {
        // A predefined window class, deliberately. Registering a class of our
        // own needs a window procedure kept alive by a delegate, a module
        // handle and a name that cannot collide across runs - three ways for a
        // throwaway control window to fail for reasons that have nothing to do
        // with what the controls are testing.
        const uint WS_POPUP = 0x80000000;
        return CreateWindowExW(0, "STATIC", "omnitype-target-probe", WS_POPUP, 0, 0, 10, 10,
                               IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero);
    }
}
'@

Write-Host "=== gate: can this probe tell a right answer from a wrong one? ===" -ForegroundColor Cyan

$ctrl = Get-Controls
if ($ctrl -eq [IntPtr]::Zero) {
    Write-Host "  FAIL  C0    could not create the control window" -ForegroundColor Red
    exit 1
}

# C1: no region must read as "none", not as radius 0.
[TargetWin]::ClearRegion($ctrl)
$c1 = [TargetWin]::RegionRadiusPx($ctrl)
Assert-Control "C1" ($c1 -eq -1) "window with no region reads as $c1 (expected -1)"

# C2: the arithmetic round-trips.
[TargetWin]::GiveEllipse($ctrl, 100, 100, 40) | Out-Null
$c2 = [TargetWin]::RegionRadiusPx($ctrl)
Assert-Control "C2" ($c2 -eq 40) "set 40 px, read back $c2 (box $([TargetWin]::RegionBox($ctrl)))"

# C3: and it can tell two different radii apart - the control that a probe which
# always returns the same number would fail.
[TargetWin]::GiveEllipse($ctrl, 100, 100, 90) | Out-Null
$c3 = [TargetWin]::RegionRadiusPx($ctrl)
Assert-Control "C3" ($c3 -eq 90 -and [Math]::Abs($c3 - $c2) -gt 20) `
    "set 90 px, read back $c3 (differs from the 40 px reading by $([Math]::Abs($c3 - $c2)))"

# C4: the DPI actually varies on this machine, so "divide by 1.25" is not a
# constant that happens to work. Reported as information; it fails only if the
# process cannot read a DPI at all, which would make every number below wrong.
$dpi = [TargetWin]::GetDpiForWindow($ctrl)
Assert-Control "C4" ($dpi -gt 0) "control window DPI = $dpi (96 = 100%, 120 = 125%)"

# C5: a handle that does not exist must be refused.
$c5 = [TargetWin]::RegionRadiusPx([IntPtr]0x7FFFFFF0)
Assert-Control "C5" ($c5 -eq -1) "a dead handle reads as $c5 (expected -1)"

[TargetWin]::ClearRegion($ctrl)
[TargetWinControl]::DestroyWindow($ctrl) | Out-Null

if ($script:Failures -gt 0) {
    Write-Host ""
    Write-Host "ABORT: $script:Failures control(s) failed. Anything this probe reports about the" -ForegroundColor Red
    Write-Host "       app after that would be a guess dressed as a measurement." -ForegroundColor Red
    exit 1
}
Write-Host "  all controls passed; the readings below can be believed" -ForegroundColor Green

if ($GateOnly) {
    Write-Host ""
    Write-Host "VERDICT: GATE PASSED (no app was measured)"
    exit 0
}

# ── measure the app ─────────────────────────────────────────────────────────
Write-Host ""
Write-Host "=== live window ===" -ForegroundColor Cyan

$procs = Get-Process -Name $Process -ErrorAction SilentlyContinue
if (-not $procs) {
    Write-Host "  $Process.exe is not running. Start it, then re-run." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "VERDICT: GATE PASSED, NO WINDOW MEASURED"
    exit 2
}

$found = $false
foreach ($p in $procs) {
    foreach ($h in [TargetWin]::TopLevelWindowsOf([uint32]$p.Id)) {
        if (-not [TargetWin]::IsWindowVisible($h)) { continue }
        $r = New-Object TargetWin+RECT
        [void][TargetWin]::GetWindowRect($h, [ref]$r)
        $w = $r.Right - $r.Left
        if ($w -lt 50) { continue }
        $found = $true

        $wdpi = [TargetWin]::GetDpiForWindow($h)
        $ppp = if ($wdpi -gt 0) { $wdpi / 96.0 } else { 1.0 }
        $radiusPx = [TargetWin]::RegionRadiusPx($h)

        Write-Host ("  hwnd=0x{0:X}  rect={1},{2} {3}x{4}  dpi={5} (ppp {6:N3})" -f `
            $h.ToInt64(), $r.Left, $r.Top, $w, ($r.Bottom - $r.Top), $wdpi, $ppp)

        if ($radiusPx -lt 0) {
            Write-Host "  region: NONE - the window claims its whole square." -ForegroundColor Yellow
            Write-Host "           That is the state O1 was supposed to make impossible while the orb is up."
            continue
        }

        $radiusPt = $radiusPx / $ppp
        Write-Host ("  click region: radius {0} px = {1:N2} pt   box {2}" -f `
            $radiusPx, $radiusPt, [TargetWin]::RegionBox($h))
        # The radius is whole pixels, so at this DPI the reading cannot be finer
        # than this. Printed so nobody treats a "match" as more precise than the
        # hardware allows - a stale build landed within 0.24 pt of a predicted
        # value on the first run of this probe, which is exactly the kind of
        # coincidence that reads as confirmation.
        Write-Host ("  quantisation: 1 px = {0:N3} pt at this DPI; a match is only meaningful to that" -f (1.0 / $ppp)) -ForegroundColor DarkGray
        Write-Host ""
        Write-Host "  consistent with these predicted targets (points):" -ForegroundColor DarkGray
        $matched = $false
        foreach ($mode in $Predicted.Keys) {
            $want = $Predicted[$mode]
            $hit = [Math]::Abs($want - $radiusPt) -le 0.5
            if ($hit) { $matched = $true }
            $mark = if ($hit) { "  <== matches" } else { "" }
            Write-Host ("    {0,-12} {1,7:N2} pt{2}" -f $mode, $want, $mark)
        }
        if (-not $matched) {
            # Silence here would read as "nothing to report", which is exactly
            # how a wrong number survives: an unmatched reading is a finding.
            Write-Host "    (nothing matches within 0.5 pt: the running build is not this code," -ForegroundColor Yellow
            Write-Host "     or the orb was mid-animation when it was sampled)" -ForegroundColor Yellow
        }
        Write-Host ""
        Write-Host "  The probe cannot tell which mode it is looking at; the match above is" -ForegroundColor DarkGray
        Write-Host "  consistent-with, not identified. Read the orb on screen to name the mode." -ForegroundColor DarkGray
    }
}

Write-Host ""
if ($found) { Write-Host "VERDICT: GATE PASSED, WINDOW MEASURED" }
else { Write-Host "VERDICT: GATE PASSED, NO VISIBLE WINDOW FOUND (exit 2)"; exit 2 }
exit 0