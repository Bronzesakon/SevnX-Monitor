@echo off
setlocal

set "PATH=%ProgramFiles%\nodejs;%PATH%"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
set "ROOT=%~dp0src-tauri"
set "FRONTEND=%~dp0frontend"
set "NPXCMD=%ProgramFiles%\nodejs\npx.cmd"
set "EXE=%~dp0src-tauri\target\debug\sevnx-monitor.exe"

echo ============================================
echo   SevnX Monitor Debug Build And Run
echo ============================================
echo.

if not exist "%NPXCMD%" (
    echo [ERROR] Node tool not found: "%NPXCMD%"
    echo Install Node.js to the default directory or edit NPXCMD in this script.
    goto :done
)

echo [1/3] TypeScript check...
cd /d "%FRONTEND%"
call "%NPXCMD%" vue-tsc --noEmit
if %ERRORLEVEL% neq 0 (
    echo [ERROR] TypeScript check failed.
    goto :done
)

echo [2/3] Frontend build...
call "%NPXCMD%" vite build
if %ERRORLEVEL% neq 0 (
    echo [ERROR] Frontend build failed.
    goto :done
)

echo [3/3] Tauri debug build...
cd /d "%ROOT%"
taskkill /IM sevnx-monitor.exe /F >nul 2>&1
cargo build
if %ERRORLEVEL% neq 0 (
    echo [ERROR] Rust build failed.
    goto :done
)

if not exist "%EXE%" (
    echo [ERROR] Debug executable not found: "%EXE%"
    goto :done
)

echo.
echo Starting app...
echo Debug logs are printed in this window and also written to the app log directory.
call "%EXE%"
echo.
echo App exited with code: %ERRORLEVEL%

:done
echo.
pause
