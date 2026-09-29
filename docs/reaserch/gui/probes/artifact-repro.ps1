<#
.SYNOPSIS
  Deterministically reproduce and measure the pale strip at the top of the orb window.

.DESCRIPTION
  The artifact is only visible against a *dark, uniform* backdrop, and the
  window has to have been up for a few seconds (winit re-applies its window
  attributes shortly after creation). Measuring it from a screenshot the user
  happened to take gave three different wrong answers in a row, so this script
  builds the conditions itself:

    1. opens a plain (never-topmost) 1920x1040 window filled with #141620 — a
       normal window can never be above a topmost one, so the orb stays on top
       of it and everything behind the orb is the same colour everywhere,
    2. launches voice-ptt with a chosen OMNITYPE_TRANSPARENCY value,
    3. samples row 4 (inside the strip) and row 40 (below it) at the window's
       horizontal centre and prints both, plus a YES/no verdict.

  Run it once per transparency mode to get a comparable matrix.

  What it proved, and where that leaves the hunt: the strip is 28 physical rows
  tall with a 1 px separator under it — the Windows 11 small-caption height at
  125% — and it is *identical* in every mode, so none of the DWM attributes
  (`DwmExtendFrameIntoClientArea`, `DWMWA_NCRENDERING_POLICY`,
  `DWMWA_SYSTEMBACKDROP_TYPE`, `DWMWA_BORDER_COLOR`) is responsible. See
  docs/GUI-WINDOW-ARTIFACT-REPORT.md §13.

.EXAMPLE
  powershell.exe -NoProfile -ExecutionPolicy Bypass -Sta -File artifact-repro.ps1
  powershell.exe -NoProfile -ExecutionPolicy Bypass -Sta -File artifact-repro.ps1 -Modes swapchain,dwm-extend-0
#>
param(
    [string[]]$Modes = @("", "dwm-extend-0", "dwm-extend"),
    [string]$Exe = "",
    [int]$StripRows = 28
)

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;using System.Runtime.InteropServices;using System.Text;
public static class OmniRepro {
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    public delegate bool EnumProc(IntPtr h, IntPtr p);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    public static IntPtr Orb = IntPtr.Zero;
    public static void FindOrb(uint pid) {
        EnumWindows(delegate(IntPtr h, IntPtr p) {
            uint q; GetWindowThreadProcessId(h, out q);
            if (q != pid) return true;
            var sb = new StringBuilder(256); GetClassNameW(h, sb, 256);
            if (sb.ToString() == "Window Class") {
                RECT r; GetWindowRect(h, out r);
                if ((r.Right - r.Left) * (r.Bottom - r.Top) > 1000) Orb = h;
            }
            return true;
        }, IntPtr.Zero);
    }
    public static RECT Rect(IntPtr h) { RECT r; GetWindowRect(h, out r); return r; }
}
"@
[OmniRepro]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null

if (-not $Exe) {
    # $PSScriptRoot = v-2/docs/reaserch/gui/probes -> up four is v-2
    $root = $PSScriptRoot
    1..4 | ForEach-Object { $root = Split-Path $root -Parent }
    $guess = Join-Path $root "voice-ptt-dist\voice-ptt.exe"
    $Exe = (Resolve-Path -LiteralPath $guess -ErrorAction SilentlyContinue)
    if (-not $Exe) { $Exe = $guess }
}
if (-not (Test-Path -LiteralPath $Exe)) { Write-Output "voice-ptt.exe not found: $Exe"; exit 1 }
$dir = Split-Path $Exe

# Uniform dark backdrop. Deliberately NOT topmost.
$bg = New-Object System.Windows.Forms.Form
$bg.FormBorderStyle = 'None'
$bg.BackColor = [System.Drawing.Color]::FromArgb(20, 22, 28)
$bg.StartPosition = 'Manual'
$bg.Location = New-Object System.Drawing.Point(0, 0)
$bg.Size = New-Object System.Drawing.Size(1920, 1040)
$bg.Show()
$bg.BringToFront()
Start-Sleep -Milliseconds 600
[System.Windows.Forms.Application]::DoEvents()

function Pump([int]$ms) {
    $end = (Get-Date).AddMilliseconds($ms)
    while ((Get-Date) -lt $end) {
        [System.Windows.Forms.Application]::DoEvents()
        Start-Sleep -Milliseconds 30
    }
}

function Measure-Mode($mode) {
    Get-Process voice-ptt -ErrorAction SilentlyContinue | Stop-Process -Force
    Pump 900
    if ($mode) { $env:OMNITYPE_TRANSPARENCY = $mode } else { $env:OMNITYPE_TRANSPARENCY = $null }
    Start-Process -FilePath $Exe -WorkingDirectory $dir
    Pump 5000
    $p = Get-Process voice-ptt -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $p) { Write-Output ("{0,-20} app did not start" -f $mode); return }
    [OmniRepro]::FindOrb([uint32]$p.Id)
    if ([OmniRepro]::Orb -eq [IntPtr]::Zero) { Write-Output ("{0,-20} orb window not found" -f $mode); return }
    $r = [OmniRepro]::Rect([OmniRepro]::Orb)

    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap($b.Width, $b.Height)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.X, $b.Y, 0, 0, $bmp.Size)
    $g.Dispose()

    $mx = $r.Left + [int](($r.Right - $r.Left) / 2)
    $inside = $bmp.GetPixel($mx, $r.Top + 4)
    $below = $bmp.GetPixel($mx, $r.Top + $StripRows + 12)

    # Walk down the first column of the window to find where the strip ends.
    $lastLit = -1
    for ($dy = 0; $dy -lt 60; $dy++) {
        $c = $bmp.GetPixel($r.Left + 6, $r.Top + $dy)
        if (($c.B - $c.R) -gt 10) { $lastLit = $dy }
    }
    $bmp.Dispose()

    $label = if ($mode) { $mode } else { 'swapchain(default)' }
    Write-Output ("{0,-20} window={1}x{2}  row4=({3},{4},{5})  row{6}=({7},{8},{9})  strip={10} rows={11}" -f `
        $label, ($r.Right - $r.Left), ($r.Bottom - $r.Top), `
        $inside.R, $inside.G, $inside.B, ($StripRows + 12), $below.R, $below.G, $below.B, `
        $(if (($inside.B - $inside.R) -gt 10) { "YES" } else { "no " }), ($lastLit + 1))
}

foreach ($m in $Modes) { Measure-Mode $m }

# leave the app running in its normal default mode
Get-Process voice-ptt -ErrorAction SilentlyContinue | Stop-Process -Force
Pump 800
$env:OMNITYPE_TRANSPARENCY = $null
Start-Process -FilePath $Exe -WorkingDirectory $dir
$bg.Close()
Write-Output "app relaunched in default mode"
