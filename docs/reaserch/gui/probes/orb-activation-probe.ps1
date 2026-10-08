# Does the running app's orb window still carry WS_EX_NOACTIVATE?
#
# Read-only by default: it only asks Windows for the ex-style of the app's own
# top-level windows and which window is in front. It never clicks or moves
# anything, so it is safe to run while the user is dictating.
#
# -Apply re-applies the bit to the live window. That is a *measurement*, not a
# fix: it lets a click on the orb be tested right now, on an instance that was
# built before the per-frame repair, and the answer is only about the mechanism
# (does a missing bit explain the stolen caret?). It lasts until the app is
# restarted or winit writes the window's styles again.
#
# Why it exists: a click on the orb must not take the foreground. The style is
# applied once at startup, and this app's transparency mode ("nostyle") never
# repairs styles afterwards — so "is the bit still there after the app has been
# running a while" is a question only a live probe can answer.

Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public class OrbProbe {
    public delegate bool EnumProc(IntPtr hwnd, IntPtr param);

    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr param);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hwnd, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hwnd, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern int GetWindowLongW(IntPtr hwnd, int index);
    [DllImport("user32.dll")] public static extern int SetWindowLongW(IntPtr hwnd, int index, int value);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);

    public static List<IntPtr> WindowsOf(uint wantedPid) {
        var found = new List<IntPtr>();
        EnumWindows((hwnd, param) => {
            uint pid;
            GetWindowThreadProcessId(hwnd, out pid);
            if (pid == wantedPid) { found.Add(hwnd); }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static string Text(IntPtr hwnd) {
        var sb = new StringBuilder(256);
        GetWindowTextW(hwnd, sb, sb.Capacity);
        return sb.ToString();
    }

    public static string Class(IntPtr hwnd) {
        var sb = new StringBuilder(256);
        GetClassNameW(hwnd, sb, sb.Capacity);
        return sb.ToString();
    }
}
"@

$Apply = $args -contains '-Apply'

$GWL_EXSTYLE = -20
$WS_EX_NOACTIVATE = 0x08000000
$WS_EX_TOPMOST = 0x00000008

$procs = Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -like 'voice-ptt*' }
if (-not $procs) { Write-Output 'VERDICT: APP_NOT_RUNNING'; exit 1 }

$foreground = [OrbProbe]::GetForegroundWindow()

foreach ($proc in $procs) {
    Write-Output ("process pid={0} started={1:yyyy-MM-dd HH:mm:ss}" -f $proc.Id, $proc.StartTime)
    $windows = [OrbProbe]::WindowsOf([uint32]$proc.Id)
    if ($windows.Count -eq 0) { Write-Output "  (no top-level windows)"; continue }
    foreach ($hwnd in $windows) {
        if ($Apply -and [OrbProbe]::Class($hwnd) -eq 'Window Class' -and [OrbProbe]::IsWindowVisible($hwnd)) {
            $before = [OrbProbe]::GetWindowLongW($hwnd, $GWL_EXSTYLE)
            $wanted = $before -bor $WS_EX_NOACTIVATE
            [OrbProbe]::SetWindowLongW($hwnd, $GWL_EXSTYLE, $wanted) | Out-Null
            $after = [OrbProbe]::GetWindowLongW($hwnd, $GWL_EXSTYLE)
            Write-Output ("  APPLIED hwnd={0} ex-before=0x{1:X8} ex-after=0x{2:X8}" -f $hwnd, $before, $after)
        }
        $ex = [OrbProbe]::GetWindowLongW($hwnd, $GWL_EXSTYLE)
        $noactivate = ($ex -band $WS_EX_NOACTIVATE) -ne 0
        $topmost = ($ex -band $WS_EX_TOPMOST) -ne 0
        $isForeground = ($hwnd -eq $foreground)
        Write-Output ("  hwnd={0} class={1} visible={2} topmost={3} NOACTIVATE={4} foreground={5} title={6}" -f `
            $hwnd, [OrbProbe]::Class($hwnd), [OrbProbe]::IsWindowVisible($hwnd), $topmost, $noactivate, $isForeground, `
            ("'" + [OrbProbe]::Text($hwnd) + "'"))
    }
}

$fgPid = 0
[OrbProbe]::GetWindowThreadProcessId($foreground, [ref]$fgPid) | Out-Null
Write-Output ("foreground hwnd={0} pid={1}" -f $foreground, $fgPid)
if ($procs.Id -contains [int]$fgPid) {
    Write-Output "VERDICT: APP_IS_FOREGROUND (a click or a window it opened took the foreground)"
} else {
    Write-Output "VERDICT: APP_NOT_FOREGROUND"
}
