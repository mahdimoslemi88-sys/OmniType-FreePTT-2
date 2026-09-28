@echo off
rem ---------------------------------------------------------------------------
rem  One-click CDP observation session for the Antigravity app (v2).
rem
rem  Antigravity already exposes a debug port, so this attaches automatically.
rem  While it runs, use Antigravity's voice input (its mic button).
rem
rem  NEW in v2: it injects probes\asr-fetch-hook.js into the page first, so the
rem  log now contains the EXACT request/response bytes of the language-server
rem  audio RPCs (base64 protobuf), not just their URLs.
rem
rem  IMPORTANT: before pressing the mic, check the "attached ->" line below is
rem  your PROJECT window (a conversation), not an onboarding/login window.
rem  If it is wrong, Ctrl+C and rerun with:  --target "<part of the title>"
rem ---------------------------------------------------------------------------
setlocal
where node >nul 2>nul
if errorlevel 1 (
  echo Node.js 21+ is required but was not found on PATH.
  echo Install it from https://nodejs.org  and try again.
  pause
  exit /b 1
)

echo Page targets available right now:
node "%~dp0observe-cdp.mjs" --app antigravity --list
echo.
echo Starting the recorder. Press the mic button in Antigravity now.
echo.
node "%~dp0observe-cdp.mjs" --app antigravity --duration 150 --poll 200 --hook "%~dp0probes\asr-fetch-hook.js"
echo.
echo Session finished. Look in this folder's "logs" subfolder.
echo To read the captured protobuf bodies:
echo   node "%~dp0probes\proto-dump.mjs" --jsonl "%~dp0logs\cdp-antigravity-XXXX.jsonl"
pause
