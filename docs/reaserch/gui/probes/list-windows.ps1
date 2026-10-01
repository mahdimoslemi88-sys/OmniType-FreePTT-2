param([string]$Exe = "", [int]$Seconds = 12)
$ErrorActionPreference = 'Stop'
$ScriptDir = $PSScriptRoot
if (-not $Exe) {
    $guess = Join-Path $ScriptDir "..\..\..\..\voice-ptt\target\release\voice-ptt.exe"
    $Exe = (Resolve-Path $guess).Path
}
Add-Type @"
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public struct LRECT { public int Left, Top, Right, Bottom; }
public static class LWin {
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, ref LRECT r);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  public static List<IntPtr> All(uint pid) {
    var l = new List<IntPtr>();
    EnumWindows((h, p) => { uint q; GetWindowThreadProcessId(h, out q); if (q == pid) l.Add(h); return true; }, IntPtr.Zero);
    return l;
  }
}
"@
Get-Process voice-ptt* -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 800
$proc = Start-Process -FilePath $Exe -PassThru
Write-Host "started pid $($proc.Id)"
for ($t = 0; $t -lt $Seconds; $t += 2) {
    Start-Sleep -Seconds 2
    if ($proc.HasExited) { Write-Host "process exited (code $($proc.ExitCode))"; break }
    Write-Host "--- t=$($t+2)s"
    foreach ($h in [LWin]::All([uint32]$proc.Id)) {
        $r = [LRECT]::new(); [void][LWin]::GetWindowRect($h, [ref]$r)
        $sb = New-Object System.Text.StringBuilder 256
        [void][LWin]::GetClassName($h, $sb, 256)
        $tb = New-Object System.Text.StringBuilder 256
        [void][LWin]::GetWindowTextW($h, $tb, 256)
        $w = $r.Right - $r.Left; $ht = $r.Bottom - $r.Top
        Write-Host ("    0x{0:X} {1}x{2} at ({3},{4}) vis={5} dpi={6} class={7} title='{8}'" -f `
            $h.ToInt64(), $w, $ht, $r.Left, $r.Top, [LWin]::IsWindowVisible($h), `
            [LWin]::GetDpiForWindow($h), $sb.ToString(), $tb.ToString())
    }
}
$proc | Stop-Process -Force
