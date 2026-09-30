<#
.SYNOPSIS
    Polls orb-ghost-check.ps1 on a loop and keeps only the frames where a
    ghost is actually present.

.DESCRIPTION
    The ghost only exists for a few frames around a state change, so a single
    manual run is a coin flip: five consecutive runs at idle were all "clean"
    and would have looked like proof. Polling turns "I did not catch it" into
    "I looked N times and here is the worst frame", which is the only claim
    worth making about a transient.

    Frames are only written to disk when the verdict is not clean, so a long
    run costs a few megabytes rather than a few hundred.

.EXAMPLE
    powershell -File ghost-watch.ps1 -Seconds 180
    powershell -File ghost-watch.ps1 -Seconds 60 -IntervalMs 500
#>
[CmdletBinding()]
param(
    [int]$Seconds = 120,
    [int]$IntervalMs = 700,
    [string]$OutDir = "$env:TEMP\ghost-frames"
)

$ErrorActionPreference = 'Stop'
$probe = Join-Path $PSScriptRoot 'orb-ghost-check.ps1'
if (-not (Test-Path $probe)) { Write-Host "probe not found: $probe"; exit 1 }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Write-Host "watching for $Seconds s, every $IntervalMs ms -> $OutDir"
$deadline = (Get-Date).AddSeconds($Seconds)
$n = 0
$clean = 0
$skipped = 0
$ghosts = @()

while ((Get-Date) -lt $deadline) {
    $n++
    $tmp = Join-Path $OutDir ("probe-{0:d4}.png" -f $n)
    # Exit 1 == ghost present. Redirect both streams so a probe warning does
    # not become this script's output.
    $out = & powershell -NoProfile -ExecutionPolicy Bypass -File $probe -Save $tmp 2>&1
    $text = ($out | Out-String) -replace "`r?`n", ' '

    # Key off the verdict TEXT, not the exit code: the probe also exits
    # non-zero when it simply cannot find the orb window (app not running),
    # and counting that as a ghost would fill the disk with false hits.
    if ($text -match 'VERDICT: GHOST ARC PRESENT') {
        if (Test-Path $tmp) {
            $dest = Join-Path $OutDir ("GHOST-{0:d4}.png" -f $n)
            Move-Item $tmp $dest -Force
        } else {
            $dest = '(probe wrote no image)'
        }
        $ghosts += $dest
        Write-Host ("[{0:d3}] GHOST -> {1}" -f $n, $dest)
    } elseif ($text -match 'VERDICT: clean') {
        $clean++
        Remove-Item $tmp -ErrorAction SilentlyContinue
        Write-Host ("[{0:d3}] clean" -f $n)
    } else {
        # Probe could not run (no orb window, screenshot failure). Say so once
        # rather than pretending it was a clean sample.
        $skipped++
        if ($skipped -le 2) {
            Write-Host ("[{0:d3}] SKIP  {1}" -f $n, $text.Substring(0, [Math]::Min(60, $text.Length)))
        }
    }
    Start-Sleep -Milliseconds $IntervalMs
}

Write-Host ''
Write-Host ("samples: {0}   clean: {1}   skipped: {2}   ghost frames: {3}" -f $n, $clean, $skipped, $ghosts.Count)
if ($ghosts.Count -gt 0) {
    Write-Host 'ghost frames kept:'
    $ghosts | ForEach-Object { Write-Host "  $_" }
    exit 1
}
if ($clean -eq 0) {
    # Every sample was skipped, so "no ghost" was never actually tested.
    # Saying "clean" here would be the exact unearned claim this script exists
    # to avoid.
    Write-Host 'NO USABLE SAMPLES - the orb window was never found. Start the app first.'
    exit 2
}
Write-Host ("no ghost in {0} usable samples ({1} skipped)" -f $clean, $skipped)
exit 0
