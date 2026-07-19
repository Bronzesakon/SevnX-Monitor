@echo off
setlocal

set "PATH=%ProgramFiles%\nodejs;%PATH%"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
set "ROOT=%~dp0src-tauri"
set "FRONTEND=%~dp0frontend"
set "NPXCMD=%ProgramFiles%\nodejs\npx.cmd"
set "EXE=%~dp0src-tauri\target\release\sevnx-monitor.exe"
set "NSISDIR=%~dp0src-tauri\target\release\bundle\nsis"
set "CI=false"

echo ============================================
echo   SevnX Monitor Release Build
echo ============================================
echo.

if not exist "%NPXCMD%" (
    echo [ERROR] Node tool not found: "%NPXCMD%"
    echo Install Node.js to the default directory or edit NPXCMD in this script.
    goto :done
)

echo [1/2] TypeScript check...
cd /d "%FRONTEND%"
call "%NPXCMD%" vue-tsc --noEmit
if %ERRORLEVEL% neq 0 (
    echo [ERROR] TypeScript check failed.
    goto :done
)

echo [2/2] Tauri release build and NSIS package...
cd /d "%ROOT%"
taskkill /IM sevnx-monitor.exe /F >nul 2>&1
cargo tauri build --bundles nsis
if %ERRORLEVEL% neq 0 (
    echo [ERROR] Tauri release build failed.
    goto :done
)

if not exist "%EXE%" (
    echo [ERROR] Release executable not found: "%EXE%"
    goto :done
)

if not exist "%NSISDIR%\*.exe" (
    echo [ERROR] NSIS installer not found in: "%NSISDIR%"
    goto :done
)

echo.
echo Release build completed: "%EXE%"
echo NSIS installer: "%NSISDIR%"

:done
echo.
pause
