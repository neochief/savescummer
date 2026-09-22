@echo off
setlocal
set "ROOT=%~dp0"
set "PS=pwsh"
where pwsh >nul 2>nul || set "PS=powershell"
"%PS%" -NoProfile -ExecutionPolicy Bypass -File "%ROOT%bin\register-explorer.ps1" -Uninstall
if errorlevel 1 (
    echo.
    echo Explorer integration was not disabled.
) else (
    echo.
    echo Explorer integration disabled. New Explorer processes load the change.
)
pause