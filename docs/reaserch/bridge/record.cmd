@echo off
rem ---------------------------------------------------------------------------
rem  One-click ASR observation session.
rem  Just double-click this file, then dictate into the Gemini / ChatGPT app.
rem  It runs for 120 seconds and writes logs\asr-observation-*.jsonl + summary.
rem  Read-only: it never injects input and never records audio.
rem ---------------------------------------------------------------------------
setlocal
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0observe-asr.ps1" -DurationSec 120 -IntervalMs 400 -ResolveDns
echo.
echo Session finished. Look in this folder's "logs" subfolder.
pause
