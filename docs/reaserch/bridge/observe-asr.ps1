<#
.SYNOPSIS
  ASR observation probe - records how the ChatGPT / Codex / Gemini desktop apps
  behave while they turn speech into text.

.DESCRIPTION
  Run this, then dictate into one of the apps. Every tick (default 2x/second) it
  records:

    * the foreground window and the process that owns it
    * each target app's processes, CPU and working set
    * live TCP connections per PID  ->  which cloud STT endpoint is contacted
    * microphone last-used timestamps (HKCU CapabilityAccessManager)  ->  exactly
      when an app grabs / releases the mic
    * the text currently sitting in the app's input box, via UI Automation
      ->  proves whether the transcript can be harvested without any injection
    * (once) loaded modules matching whisper/onnx/ggml/vosk/... -> is there ANY
      local model, or is it 100% cloud?

  Output: <OutDir>\asr-observation-<stamp>.jsonl  (machine readable, one JSON
  object per line) plus a matching -summary.md written at the end.
  Press <Enter> during the run to drop a marker (e.g. "started speaking").

  This script is READ-ONLY. It does not inject input, does not touch the
  network, and never reads audio. It only observes.

.PARAMETER Apps
  Process base names to watch. Default: ChatGPT, Codex, Gemini.

.PARAMETER IntervalMs
  Sampling period in milliseconds. Default 500.

.PARAMETER DurationSec
  Auto-stop after N seconds. 0 = run until Ctrl+C.

.PARAMETER OutDir
  Where logs are written. Default: .\logs next to this script.

.PARAMETER ResolveDns
  Reverse-resolve remote connection IPs (slower, identifies openai.com /
  google.com endpoints).

.PARAMETER NoUia
  Skip the UI Automation text sampling (if it misbehaves on your machine).

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File observe-asr.ps1 -DurationSec 180 -ResolveDns
#>
[CmdletBinding()]
param(
    [string[]]$Apps = @('ChatGPT', 'Codex', 'Gemini'),
    [int]$IntervalMs = 500,
    [int]$DurationSec = 0,
    [string]$OutDir = (Join-Path $PSScriptRoot 'logs'),
    [switch]$ResolveDns,
    [switch]$NoUia
)

$ErrorActionPreference = 'Continue'
$ProgressPreference = 'SilentlyContinue'

# ---- optional UIA assemblies -------------------------------------------------
if (-not $NoUia) {
    foreach ($asm in 'UIAutomationClient', 'UIAutomationTypes') {
        try { Add-Type -AssemblyName $asm -ErrorAction Stop } catch { $NoUia = $true }
    }
}

# ---- Win32 helpers -----------------------------------------------------------
try {
    Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class ObserveWin32 {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr hWnd, out int pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int count);
}
'@ -ErrorAction Stop
} catch {
    Write-Warning "Could not compile Win32 helpers: $_"
}

if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$jsonlPath = Join-Path $OutDir "asr-observation-$stamp.jsonl"
$mdPath = Join-Path $OutDir "asr-observation-$stamp-summary.md"

$script:events = New-Object System.Collections.ArrayList
$script:lastUia = @{}
$script:dnsCache = @{}
$script:knownEndpoint = @{
    '142.250.' = 'Google'
    '172.217.' = 'Google'
    '216.58.'  = 'Google'
    '74.125.'  = 'Google'
    '104.18.'  = 'Cloudflare (often OpenAI)'
    '13.107.'  = 'Microsoft'
}

function Write-Event {
    param([hashtable]$Event)
    $Event['ts'] = (Get-Date).ToString('o')
    $line = ($Event | ConvertTo-Json -Compress -Depth 6)
    Add-Content -LiteralPath $jsonlPath -Value $line -Encoding UTF8
    [void]$script:events.Add($Event)
}

function Get-ForegroundInfo {
    try {
        $h = [ObserveWin32]::GetForegroundWindow()
        $procId = 0
        [void][ObserveWin32]::GetWindowThreadProcessId($h, [ref]$procId)
        $sb = New-Object System.Text.StringBuilder 512
        [void][ObserveWin32]::GetWindowTextW($h, $sb, 512)
        $name = ''
        try { $name = (Get-Process -Id $procId -ErrorAction Stop).ProcessName } catch {}
        return [ordered]@{ hwnd = [int64]$h; pid = $procId; process = $name; title = $sb.ToString() }
    } catch {
        return [ordered]@{ hwnd = 0; pid = 0; process = ''; title = '' }
    }
}

function Get-TargetProcesses {
    $list = New-Object System.Collections.ArrayList
    foreach ($a in $Apps) {
        foreach ($p in (Get-Process -Name $a -ErrorAction SilentlyContinue)) {
            $cpu = $null
            try { $cpu = [math]::Round($p.CPU, 2) } catch {}
            [void]$list.Add([ordered]@{
                pid     = $p.Id
                name    = $p.ProcessName
                cpuSec  = $cpu
                wsMB    = [math]::Round($p.WorkingSet64 / 1MB, 1)
                window  = $p.MainWindowTitle
            })
        }
    }
    return $list
}

function Get-CmdLineMap {
    $map = @{}
    try {
        Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
            Where-Object { $Apps -contains ($_.Name -replace '\.exe$', '') } |
            ForEach-Object { $map[[int]$_.ProcessId] = $_.CommandLine }
    } catch {}
    return $map
}

function Get-CdpPortFromCmdLine {
    param([string]$CmdLine)
    if ($CmdLine -match '--remote-debugging-port=(\d+)') { return [int]$Matches[1] }
    return $null
}

function Test-CdpEndpoint {
    param([int]$Port)
    try {
        $r = Invoke-RestMethod -Uri "http://127.0.0.1:$Port/json/version" -TimeoutSec 1 -ErrorAction Stop
        return ($r | ConvertTo-Json -Compress -Depth 4)
    } catch { return $null }
}

function Get-Connections {
    # netstat (~40 ms) is dramatically faster than Get-NetTCPConnection -OwningProcess
    # (~1.1 s per PID), which matters when sampling several times per second.
    param([int[]]$Pids)
    $out = New-Object System.Collections.ArrayList
    if (-not $Pids -or $Pids.Count -eq 0) { return $out }
    $set = @{}
    foreach ($p in $Pids) { $set[[string]$p] = $true }

    $raw = $null
    try { $raw = & netstat -ano 2>$null } catch { return $out }

    foreach ($line in $raw) {
        if ($line -notmatch '^\s*(TCP|UDP)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\d+)\s*$') { continue }
        $proto  = $Matches[1]
        $local  = $Matches[2]
        $remote = $Matches[3]
        $state  = $Matches[4]
        $procId = $Matches[5]
        if (-not $set.ContainsKey($procId)) { continue }
        if ($remote -match '^(0\.0\.0\.0|127\.0\.0\.1|\[::\]|\[::1\]|::):') { continue }
        if ($remote -eq '*:*') { continue }

        $addr = $remote
        if ($addr.StartsWith('[')) { $addr = $addr.Substring(0, $addr.IndexOf(']')) }
        else { $addr = ($addr -split ':')[0] }

        $rdns = $null
        if ($ResolveDns) {
            if ($script:dnsCache.ContainsKey($addr)) {
                $rdns = $script:dnsCache[$addr]
            } else {
                try {
                    $rdns = (Resolve-DnsName -Type PTR -Name $addr -ErrorAction Stop |
                             Select-Object -First 1 -ExpandProperty NameHost)
                } catch { $rdns = $null }
                $script:dnsCache[$addr] = $rdns
            }
        }
        $vendor = $null
        foreach ($pref in $script:knownEndpoint.Keys) {
            if ($addr.StartsWith($pref)) { $vendor = $script:knownEndpoint[$pref]; break }
        }

        [void]$out.Add([ordered]@{
            pid    = [int]$procId
            proto  = $proto
            local  = $local
            remote = $remote
            state  = $state
            rdns   = $rdns
            vendor = $vendor
        })
    }
    return $out
}

function Get-MicUsage {
    $map = @{}
    $bases = @(
        'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged',
        'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\Packaged'
    )
    foreach ($b in $bases) {
        if (-not (Test-Path $b)) { continue }
        foreach ($k in (Get-ChildItem $b -ErrorAction SilentlyContinue)) {
            $p = Get-ItemProperty $k.PSPath -ErrorAction SilentlyContinue
            if ($null -eq $p -or $null -eq $p.LastUsedTimeStart) { continue }
            try { $start = [DateTime]::FromFileTime([int64]$p.LastUsedTimeStart) } catch { continue }
            $stopRaw = 0
            if ($null -ne $p.LastUsedTimeStop) { $stopRaw = [int64]$p.LastUsedTimeStop }
            $stopStr = $null
            if ($stopRaw -ne 0) { try { $stopStr = [DateTime]::FromFileTime($stopRaw).ToString('o') } catch {} }
            $map[$k.PSChildName] = [ordered]@{
                start = $start.ToString('o')
                stop  = $stopStr
                inUse = ($stopRaw -eq 0)
            }
        }
    }
    return $map
}

function Get-ElementText {
    param($Element)
    try {
        $vp = $Element.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
        if ($vp -and $vp.Current.Value) { return [string]$vp.Current.Value }
    } catch {}
    try {
        $tp = $Element.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern)
        if ($tp) {
            $s = $tp.DocumentRange.GetText(4000)
            if ($s) { return [string]$s }
        }
    } catch {}
    try { if ($Element.Current.Name) { return [string]$Element.Current.Name } } catch {}
    return $null
}

function Get-UiaTexts {
    param([int]$ProcessId, [int64]$Hwnd = 0, [int]$Max = 6)
    if ($NoUia) { return @() }
    $out = New-Object System.Collections.ArrayList
    try {
        if ($Hwnd -ne 0) {
            # Fast path: we already know the focused top-level window handle.
            $w0 = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$Hwnd)
            $wins = if ($w0) { @($w0) } else { @() }
        } else {
            $root = [System.Windows.Automation.AutomationElement]::RootElement
            $cond = New-Object System.Windows.Automation.PropertyCondition(
                [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $ProcessId)
            $wins = $root.FindAll([System.Windows.Automation.TreeScope]::Children, $cond)
        }
        foreach ($w in $wins) {
            foreach ($ct in @([System.Windows.Automation.ControlType]::Edit,
                              [System.Windows.Automation.ControlType]::Document)) {
                $tc = New-Object System.Windows.Automation.PropertyCondition(
                    [System.Windows.Automation.AutomationElement]::ControlTypeProperty, $ct)
                $els = $w.FindAll([System.Windows.Automation.TreeScope]::Descendants, $tc)
                foreach ($el in $els) {
                    $t = Get-ElementText $el
                    if ($t) { [void]$out.Add($t.Trim()) }
                    if ($out.Count -ge $Max) { return @($out) }
                }
            }
        }
    } catch {}
    return @($out)
}

function Get-InterestingModules {
    param([int]$ProcessId)
    $pat = 'whisper|onnx|ggml|vosk|sherpa|ctranslate|torch|speech|webrtc|electron'
    try {
        return @(Get-Process -Id $ProcessId -Module -ErrorAction Stop |
                 Where-Object { $_.ModuleName -match $pat } |
                 Select-Object -ExpandProperty ModuleName -Unique)
    } catch { return @() }
}

# ------------------------------------------------------------------ session ---
$targetsNow = Get-TargetProcesses
if ($targetsNow.Count -eq 0) {
    Write-Warning "None of [$($Apps -join ', ')] are running. Start the app first, then re-run."
}

Write-Host ""
Write-Host "  ==============================================================" -ForegroundColor Cyan
Write-Host "   ASR OBSERVATION PROBE - recording is running" -ForegroundColor Cyan
Write-Host "  ==============================================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "   Watching : $($Apps -join ', ')"
Write-Host "   Interval : ${IntervalMs}ms"
Write-Host "   Log file : $jsonlPath"
Write-Host ""
Write-Host "   WHAT TO DO NOW:" -ForegroundColor Yellow
Write-Host "     1. Switch to the Gemini (or ChatGPT) window."
Write-Host "     2. Press its microphone button and dictate a short sentence."
Write-Host "     3. When the text appears, press <Enter> in THIS window to drop a"
Write-Host "        marker, so we can match the log to what you did."
Write-Host "     4. Repeat 3-4 times, then come back and press Ctrl+C."
Write-Host ""
Write-Host "   It stops by itself after $DurationSec s (0 = until Ctrl+C)." -ForegroundColor DarkGray
Write-Host ""

Write-Event @{
    type = 'session_start'
    apps = $Apps
    intervalMs = $IntervalMs
    durationSec = $DurationSec
    uia = (-not $NoUia)
    host = $env:COMPUTERNAME
    user = $env:USERNAME
}

# one-time inventory of running targets + local-model detection + CDP probing
$cmdMap = Get-CmdLineMap
foreach ($p in $targetsNow) {
    $cmd = $cmdMap[[int]$p.pid]
    $cdp = if ($cmd) { Get-CdpPortFromCmdLine -CmdLine $cmd } else { $null }
    Write-Event @{
        type       = 'target_inventory'
        pid        = $p.pid
        name       = $p.name
        wsMB       = $p.wsMB
        modules    = (Get-InterestingModules -ProcessId $p.pid)
        cdpFlag    = $cdp
        cdpVersion = $(if ($cdp) { Test-CdpEndpoint -Port $cdp } else { $null })
        cmdLine    = $(if ($cmd) { $cmd.Substring(0, [Math]::Min(400, $cmd.Length)) } else { $null })
    }
}
foreach ($port in 9222, 9223) {
    $v = Test-CdpEndpoint -Port $port
    if ($v) { Write-Event @{ type = 'cdp_open'; port = $port; version = $v } }
}

$start = Get-Date
$tick = 0
$stop = $false

while (-not $stop) {
    $tick++
    $now = Get-Date
    $sw = [System.Diagnostics.Stopwatch]::StartNew()

    # marker on Enter
    try {
        while ([Console]::KeyAvailable) {
            $key = [Console]::ReadKey($true)
            if ($key.Key -eq 'Enter') {
                Write-Host "  [marker @ $($now.ToString('HH:mm:ss.fff'))]" -ForegroundColor Yellow
                Write-Event @{ type = 'marker'; note = 'user pressed Enter' }
            }
        }
    } catch {}

    $procs = Get-TargetProcesses
    $pids = @($procs | ForEach-Object { [int]$_.pid })
    $fg = Get-ForegroundInfo

    # UI Automation is the expensive probe, and dictation text only matters for
    # the window that is actually focused -- so sample it there and nowhere else.
    $uiaMap = @{}
    if (-not $NoUia) {
        $focusedTarget = $procs | Where-Object { [int]$_.pid -eq [int]$fg.pid } | Select-Object -First 1
        if ($focusedTarget) {
            $texts = Get-UiaTexts -ProcessId $fg.pid -Hwnd $fg.hwnd
            $joined = ($texts -join " || ")
            if ($joined -and $script:lastUia[$fg.pid] -ne $joined) {
                $script:lastUia[$fg.pid] = $joined
                $uiaMap[[string]$fg.pid] = $texts
            }
        }
    }

    Write-Event @{
        type        = 'snapshot'
        tick        = $tick
        foreground  = $fg
        processes   = @($procs)
        mic         = (Get-MicUsage)
        connections = @(Get-Connections -Pids $pids)
        newText     = $uiaMap
        tickMs      = $sw.ElapsedMilliseconds
    }

    if ($DurationSec -gt 0 -and ((Get-Date) - $start).TotalSeconds -ge $DurationSec) { $stop = $true }
    if (-not $stop) { Start-Sleep -Milliseconds $IntervalMs }
}

Write-Event @{ type = 'session_end'; ticks = $tick; jsonl = $jsonlPath }

# --------------------------------------------------------------- summary -----
$sb = New-Object System.Text.StringBuilder
[void]$sb.AppendLine("# ASR observation summary - $stamp")
[void]$sb.AppendLine()
[void]$sb.AppendLine("Watched: $($Apps -join ', ')   |   ticks: $tick   |   interval: ${IntervalMs}ms")
[void]$sb.AppendLine()
[void]$sb.AppendLine("| time | event | detail |")
[void]$sb.AppendLine("|---|---|---|")
foreach ($e in $script:events) {
    $t = if ($e.ts) { ([datetime]$e.ts).ToString('HH:mm:ss.fff') } else { '' }
    switch ($e.type) {
        'target_inventory' {
            $mods = if ($e.modules) { ($e.modules -join ',') } else { '-' }
            [void]$sb.AppendLine("| $t | inventory | pid=$($e.pid) $($e.name) ws=$($e.wsMB)MB cdp=$($e.cdpFlag) mods=$mods |")
        }
        'cdp_open' { [void]$sb.AppendLine("| $t | **CDP OPEN** | port=$($e.port) |") }
        'marker'   { [void]$sb.AppendLine("| $t | **MARKER** | $($e.note) |") }
        'snapshot' {
            if ($e.newText.PSObject.Properties.Count -gt 0) {
                foreach ($prop in $e.newText.PSObject.Properties) {
                    $val = ($prop.Value -join ' / ')
                    [void]$sb.AppendLine("| $t | **TEXT** | pid=$($prop.Name): $val |")
                }
            }
            if ($e.foreground -and $e.foreground.process -match ($Apps -join '|')) {
                [void]$sb.AppendLine("| $t | focus | $($e.foreground.process): $($e.foreground.title) |")
            }
        }
    }
}
$sb.ToString() | Set-Content -LiteralPath $mdPath -Encoding UTF8

Write-Host ""
Write-Host "Done. $tick snapshots." -ForegroundColor Green
Write-Host "  JSONL   : $jsonlPath"
Write-Host "  Summary : $mdPath"
