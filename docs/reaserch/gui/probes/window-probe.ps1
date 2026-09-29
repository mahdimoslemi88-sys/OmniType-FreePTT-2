<#
.SYNOPSIS
  OmniType FreePTT GUI window/memory probe.

.DESCRIPTION
  Enumerates every top-level Win32 window owned by the running voice-ptt.exe
  process (handle, class, title, rect, visibility, ex-style, owner) and reads
  the process working set / private bytes. Use it to reproduce the reported
  "ghost boxes accumulate above the orb" bug and to prove a fix: count windows
  per class before and after a few dictation rounds.

  Read-only: it calls nothing but EnumWindows/GetWindow* and Get-Process.

.EXAMPLE
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File window-probe.ps1
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File window-probe.ps1 -Samples 6 -IntervalSec 5
#>
param(
    [int]$TargetPid = 0,
    [int]$Samples = 1,
    [int]$IntervalSec = 3
)

Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class OmniWinProbe
{
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr hWnd, int index);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr hWnd, uint cmd);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hWnd);

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    public static List<string> Snapshot(uint target, out int count)
    {
        var lines = new List<string>();
        int seen = 0;
        EnumWindows(delegate(IntPtr h, IntPtr lp)
        {
            uint pid;
            GetWindowThreadProcessId(h, out pid);
            if (pid == target)
            {
                var cls = new StringBuilder(256);
                GetClassNameW(h, cls, cls.Capacity);
                var title = new StringBuilder(256);
                GetWindowTextW(h, title, title.Capacity);
                RECT r;
                GetWindowRect(h, out r);
                int ex = GetWindowLong(h, -20);
                IntPtr owner = GetWindow(h, 4 /* GW_OWNER */);
                lines.Add(string.Format(
                    "hwnd=0x{0:X8} class={1,-24} visible={2,-5} iconic={3,-5} ex=0x{4:X8} owner=0x{5:X8} rect=({6},{7} {8}x{9}) title=\"{10}\"",
                    h.ToInt64(), cls.ToString(), IsWindowVisible(h), IsIconic(h), ex, owner.ToInt64(),
                    r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top, title.ToString()));
                seen++;
            }
            return true;
        }, IntPtr.Zero);
        count = seen;
        lines.Sort();
        return lines;
    }
}
"@

function Get-VoicePttProcess {
    param([int]$Id)
    if ($Id -gt 0) { return Get-Process -Id $Id -ErrorAction SilentlyContinue }
    return Get-Process -Name voice-ptt -ErrorAction SilentlyContinue | Select-Object -First 1
}

for ($i = 0; $i -lt $Samples; $i++) {
    $proc = Get-VoicePttProcess -Id $TargetPid
    if (-not $proc) {
        Write-Output "voice-ptt.exe is not running."
        break
    }

    $count = 0
    $lines = [OmniWinProbe]::Snapshot([uint32]$proc.Id, [ref]$count)

    Write-Output ("==== sample {0}/{1}  pid={2}  {3}" -f ($i + 1), $Samples, $proc.Id, (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
    Write-Output ("-- memory: working_set={0} MB  private={1} MB  handles={2}  threads={3}" -f `
        [math]::Round($proc.WorkingSet64 / 1MB, 1), `
        [math]::Round($proc.PrivateMemorySize64 / 1MB, 1), `
        $proc.HandleCount, $proc.Threads.Count)
    Write-Output ("-- top-level windows: {0}" -f $count)

    $byClass = @{}
    foreach ($line in $lines) {
        if ($line -match 'class=(\S+)') {
            $c = $Matches[1]
            if ($byClass.ContainsKey($c)) { $byClass[$c]++ } else { $byClass[$c] = 1 }
        }
    }
    Write-Output "-- by class:"
    $byClass.GetEnumerator() | Sort-Object Name | ForEach-Object { Write-Output ("   {0,-26} x{1}" -f $_.Key, $_.Value) }
    Write-Output "-- windows:"
    $lines | ForEach-Object { Write-Output ("   {0}" -f $_) }
    Write-Output ""

    if ($i -lt ($Samples - 1)) { Start-Sleep -Seconds $IntervalSec }
}
