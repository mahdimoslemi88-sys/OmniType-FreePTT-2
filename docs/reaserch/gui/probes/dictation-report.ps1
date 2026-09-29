<#
.SYNOPSIS
  OmniType FreePTT dictation report: turn the app log into evidence for the
  "long dictation / chunk seams / local model" fixes.

.DESCRIPTION
  Reads the daily log (`%APPDATA%\voice-ptt\logs\voice-ptt.log.YYYY-MM-DD`) and
  reports, per recording session:

    * how long the session actually ran (max `audio_secs` on a flush) - the old
      build always stopped at ~30.0 s (the ring-buffer safety valve),
    * how many mid-session chunks were flushed while the mic was still live
      (`flushing mid-session chunk ... still_recording=true`),
    * how many chunk seams were repaired and what was dropped/backspaced
      (`chunk seam repaired text=... dropped=N backspaces=M`),
    * whether the 1.6 GB local whisper model was ever loaded
      (`loading whisper model on first use`) - it must NOT appear when a cloud
      engine is selected,
    * engine outcomes and any errors/warnings, so a silent failure cannot be
      mistaken for a clean run.

  Read-only: it only reads log files.

.EXAMPLE
  # after dictating for a minute or two, with the app still running:
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File dictation-report.ps1
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File dictation-report.ps1 -Date 2026-09-29
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File dictation-report.ps1 -LogPath ..\..\..\some.log
#>
param(
    [string]$Date = (Get-Date -Format 'yyyy-MM-dd'),
    [string]$LogPath = "",
    [int]$Tail = 0
)

if (-not $LogPath) {
    $dataDir = Join-Path $env:APPDATA 'voice-ptt'
    $LogPath = Join-Path $dataDir ("logs\voice-ptt.log.$Date")
}

if (-not (Test-Path -LiteralPath $LogPath)) {
    Write-Output "log not found: $LogPath"
    Write-Output "hint: run the app once, dictate, then re-run this script (or pass -LogPath / -Date)."
    exit 1
}

$lines = Get-Content -LiteralPath $LogPath -Encoding UTF8
if ($Tail -gt 0 -and $lines.Count -gt $Tail) {
    $lines = $lines[($lines.Count - $Tail)..($lines.Count - 1)]
}

function Get-Field {
    param([string]$Line, [string]$Name)
    if ($Line -match ("(?:^|\s)" + [regex]::Escape($Name) + "=([^\s]+)")) { return $Matches[1] }
    return ""
}

$tailNote = if ($Tail -gt 0) { " (last $Tail)" } else { "" }
Write-Output "== dictation report =="
Write-Output ("log        : {0}" -f $LogPath)
Write-Output ("lines      : {0}{1}" -f $lines.Count, $tailNote)
Write-Output ""

# == sessions ================================================================
$session = 0
$sessions = New-Object System.Collections.ArrayList
$current = $null
$chunks = 0
$seams = 0
$dropped = 0
$backspaces = 0
$maxAudio = 0.0
$fragmentLines = New-Object System.Collections.ArrayList
$longUtterances = New-Object System.Collections.ArrayList
$notRecording = 0
$seamLines = New-Object System.Collections.ArrayList
$localLoads = New-Object System.Collections.ArrayList
$errors = New-Object System.Collections.ArrayList
$engineCounts = @{}

function Close-Session {
    if ($null -eq $script:current) { return }
    [void]$script:sessions.Add([pscustomobject]@{
        Index      = $script:session
        Chunks     = $script:current.Chunks
        MaxAudio   = $script:current.MaxAudio
        Seams      = $script:current.Seams
    })
}

foreach ($line in $lines) {
    # A new session starts on "recording started"; sessions also end implicitly
    # when the next one starts, so nothing has to be paired by hand.
    if ($line -match 'INFO .*: recording started') {
        Close-Session
        $script:session++
        $script:current = [pscustomobject]@{ Chunks = 0; MaxAudio = 0.0; Seams = 0 }
        continue
    }

    if ($line -match 'flushing mid-session chunk') {
        if ($null -ne $script:current) { $script:current.Chunks++ ; $script:chunks++ }
        $secs = 0.0
        [void][double]::TryParse((Get-Field $line 'audio_secs'), [ref]$secs)
        if ($secs -gt $maxAudio) { $maxAudio = $secs }
        if ($null -ne $script:current -and $secs -gt $script:current.MaxAudio) { $script:current.MaxAudio = $secs }
        if ($line -notmatch 'still_recording=true') {
            $notRecording++
            [void]$fragmentLines.Add("chunk flushed while NOT recording: $line")
        }
    }

    if ($line -match 'transcribing utterance') {
        $secs = 0.0
        [void][double]::TryParse((Get-Field $line 'audio_secs'), [ref]$secs)
        if ($secs -gt $maxAudio) { $maxAudio = $secs }
        if ($secs -ge 30.0) {
            [void]$longUtterances.Add(("whole-utterance transcribe of {0:N2}s (no chunking active?)" -f $secs))
        }
    }

    if ($line -match 'chunk seam repaired') {
        if ($null -ne $script:current) { $script:current.Seams++ ; $script:seams++ }
        $d = 0; [void][int]::TryParse((Get-Field $line 'dropped'), [ref]$d); $dropped += $d
        $b = 0; [void][int]::TryParse((Get-Field $line 'backspaces'), [ref]$b); $backspaces += $b
        [void]$seamLines.Add($line)
    }

    if ($line -match 'loading whisper model on first use') { [void]$localLoads.Add($line) }
    if ($line -match '\bERROR\b') { [void]$errors.Add($line) }
    if ($line -match 'asr success engine="([^"]+)"') {
        $engine = $Matches[1]
        if ($engineCounts.ContainsKey($engine)) { $engineCounts[$engine]++ } else { $engineCounts[$engine] = 1 }
    }
}
Close-Session

# == verdicts ================================================================
$pass = 0
$fail = 0
function Check {
    param([string]$Ok, [string]$Bad, [bool]$Condition)
    if ($Condition) { Write-Output ("  [PASS] " + $Ok); $script:pass++ }
    else { Write-Output ("  [FAIL] " + $Bad); $script:fail++ }
}

Write-Output ("sessions          : {0}" -f $sessions.Count)
foreach ($s in ($sessions | Select-Object -Last 10)) {
    Write-Output ("  session {0,-3} chunks={1,-3} max chunk audio={2:N2}s  seams repaired={3}" -f $s.Index, $s.Chunks, $s.MaxAudio, $s.Seams)
}
if ($sessions.Count -gt 10) { Write-Output "  (only the last 10 sessions are listed)" }
Write-Output ("chunks flushed    : {0}   (all mid-session, mic still live)" -f $chunks)
Write-Output ("max audio_secs    : {0:N2}   (any value > 30.0 proves the old ceiling is gone)" -f $maxAudio)
Write-Output ("seam repairs      : {0}   dropped_words={1} backspaces={2}" -f $seams, $dropped, $backspaces)
$engineText = '(none in this log)'
if ($engineCounts.Count -gt 0) {
    $engineText = (($engineCounts.GetEnumerator() | Sort-Object Name | ForEach-Object { $_.Key + '=' + $_.Value }) -join ' ')
}
Write-Output ("engines used      : {0}" -f $engineText)
Write-Output ("local model loads : {0}   (must be 0 when a cloud engine is selected)" -f $localLoads.Count)
Write-Output ("errors            : {0}" -f $errors.Count)
Write-Output ""

Write-Output "checks:"
Check "at least one recording session" "no recording session found in this log" ($sessions.Count -gt 0)
Check "every flushed chunk is mid-session (still_recording=true)" "some chunks were flushed with the mic already stopped" ($notRecording -eq 0)
Check "session ran past the old 30 s ceiling" "longest audio seen is under/at 30 s - dictate longer to prove it" ($maxAudio -gt 30.0)
Check "local whisper model was not loaded" "the 1.6 GB local model was loaded (unexpected for a cloud engine)" ($localLoads.Count -eq 0)

if ($fragmentLines.Count -gt 0) {
    Write-Output ""
    Write-Output "notes:"
    $fragmentLines | ForEach-Object { Write-Output ("   {0}" -f $_) }
}
if ($longUtterances.Count -gt 0) {
    Write-Output ""
    Write-Output "whole-utterance (unchunked) transcriptions of 30 s+:"
    $longUtterances | ForEach-Object { Write-Output ("   {0}" -f $_) }
}
if ($seamLines.Count -gt 0) {
    Write-Output ""
    Write-Output "seam repairs (last 10):"
    $seamLines | Select-Object -Last 10 | ForEach-Object { Write-Output ("   {0}" -f $_) }
}
if ($localLoads.Count -gt 0) {
    Write-Output ""
    Write-Output "local model load lines:"
    $localLoads | ForEach-Object { Write-Output ("   {0}" -f $_) }
}
if ($errors.Count -gt 0) {
    Write-Output ""
    Write-Output "errors (last 10):"
    $errors | Select-Object -Last 10 | ForEach-Object { Write-Output ("   {0}" -f $_) }
}

Write-Output ""
Write-Output ("result: {0} passed, {1} failed" -f $pass, $fail)
exit $(if ($fail -gt 0) { 1 } else { 0 })
