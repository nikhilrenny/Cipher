@echo off
cd /d "%~dp0"
npm run tauri dev
echo.
echo Cipher closed. Press any key to close this window.
pause >nul
