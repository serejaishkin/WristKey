@echo off
REM Build script for WristKey Credential Provider DLL
REM Usage: build-credential-provider.bat [Debug|Release]

set CONFIGURATION=%1
if "%CONFIGURATION%"=="" set CONFIGURATION=Release

echo Building WristKey Credential Provider (%CONFIGURATION%)...
echo.

REM Call PowerShell build script
powershell -ExecutionPolicy Bypass -File "%~dp0build-credential-provider.ps1" -Configuration %CONFIGURATION%

if %ERRORLEVEL% NEQ 0 (
    echo.
    echo Build failed!
    pause
    exit /b %ERRORLEVEL%
)

echo.
echo Build completed successfully!
echo.
echo Next steps:
echo 1. Run register.ps1 as Administrator to register the credential provider
echo 2. Ensure WristKey daemon is running before lock screen
echo 3. Restart computer or run 'shutdown /r /t 0' to apply changes
echo.
pause