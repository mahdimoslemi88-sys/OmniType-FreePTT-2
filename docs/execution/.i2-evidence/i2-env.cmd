@echo off
REM -----------------------------------------------------------------------
REM I2 step: one cargo command with MSVC/Windows SDK (vcvars64) and the
REM VS-bundled CMake on PATH. Both die with this process; nothing
REM permanent is written.
REM Usage: i2-env.cmd <cargo args...>
REM -----------------------------------------------------------------------
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul
set "PATH=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin;C:\Program Files (x86)\Microsoft Visual Studio\Installer;%PATH%"
set "CMAKE=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
cd /d "C:\Users\LENOVO LOQ\tools\OmniType-FreePTT\v-2\voice-ptt"
cargo %*
echo CARGO_EXIT=%ERRORLEVEL%
exit /b 0