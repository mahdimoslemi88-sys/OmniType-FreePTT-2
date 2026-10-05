@echo off
REM -----------------------------------------------------------------------
REM One cargo subcommand with MSVC/Windows SDK (vcvars64) and the
REM VS-bundled CMake on PATH. Both die with this process; nothing
REM permanent is written.
REM Usage: cargo-env.cmd test
REM        cargo-env.cmd clippy --lib --all-targets -- -D warnings
REM -----------------------------------------------------------------------
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul
set "PATH=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin;C:\Program Files (x86)\Microsoft Visual Studio\Installer;%PATH%"
set "CMAKE=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
cd /d "%~dp0..\..\..\voice-ptt"
cargo %*
echo CARGO_EXIT=%ERRORLEVEL%
exit /b 0
