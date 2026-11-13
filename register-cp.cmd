@echo off
setlocal EnableExtensions
rem ============================================================
rem  register-cp.cmd -- install the WristKey Credential Provider
rem  with VISIBLE output (self-elevates, never closes on error).
rem  Usage: register-cp.cmd [path\to\WristKeyCredentialProvider.dll] [path\to\app.exe]
rem ============================================================
set "ROOT=%~dp0"
set "CLSID={7E1B7B8A-4C8B-4C2F-9D8A-8D3A7F2E51A1}"
set "RC2=0"

net session >nul 2>&1
if errorlevel 1 (
    echo Requesting administrator rights...
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -ArgumentList '%1','%2' -Verb RunAs"
    exit /b
)

set "DLL=%~1"
if "%DLL%"=="" set "DLL=%ROOT%windows-credential-provider\build\Release\WristKeyCredentialProvider.dll"
if not exist "%DLL%" set "DLL=%ROOT%windows-credential-provider\build\WristKeyCredentialProvider.dll"
set "EXE=%~2"
if "%EXE%"=="" set "EXE=%ROOT%desktop\tauri\src-tauri\target\release\wristkey-tauri.exe"
if not exist "%EXE%" set "EXE=%ROOT%desktop\tauri\src-tauri\target\release\WristKey.exe"

echo.
echo DLL: %DLL%
echo EXE: %EXE%
if not exist "%DLL%" ( echo [FAIL] DLL not found. Run build-dll.cmd first. & set "RC2=1" & goto :end )
if not exist "%EXE%" ( echo [WARN] app exe not found - the service will not be created. Run: cargo tauri build )

echo.
echo [1/5] Stop service and copy DLL to System32
sc stop WristKey >nul 2>&1
copy /Y "%DLL%" "%SystemRoot%\System32\WristKeyCredentialProvider.dll"
if errorlevel 1 ( echo [FAIL] copy to System32 failed - the DLL is locked or the path is wrong. Reboot and retry. & set "RC2=1" & goto :end )

echo.
echo [2/5] Tell the DLL where the app and data live
if exist "%EXE%" reg add "HKLM\SOFTWARE\WristKey" /v ExePath /t REG_SZ /d "%EXE%" /f
reg add "HKLM\SOFTWARE\WristKey" /v DataDir /t REG_SZ /d "%APPDATA%\WristKey\data" /f
if not exist "%APPDATA%\WristKey\data" mkdir "%APPDATA%\WristKey\data"

echo.
echo [3/5] regsvr32
"%SystemRoot%\System32\regsvr32.exe" "%SystemRoot%\System32\WristKeyCredentialProvider.dll"
if errorlevel 1 ( echo [FAIL] regsvr32 exit code %errorlevel% & set "RC2=1" & goto :end )

echo.
echo [4/5] Check registration
reg query "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\%CLSID%" >nul 2>&1
if errorlevel 1 ( echo [FAIL] CP key missing. Is the CLSID in the built DLL different from %CLSID% ? & set "RC2=1" ) else ( echo [OK] Credential Providers key present )
reg query "HKLM\SOFTWARE\Classes\CLSID\%CLSID%\InprocServer32" /ve 2>nul | find /i "WristKeyCredentialProvider.dll" >nul
if errorlevel 1 ( echo [FAIL] InprocServer32 does not point at the DLL & set "RC2=1" ) else ( echo [OK] InprocServer32 -> DLL )

echo.
echo [5/5] Service
sc query WristKey
sc qc WristKey | find /i "BINARY_PATH_NAME"

:end
echo.
if "%RC2%"=="0" ( echo DONE. Press Win+L and look for the WristKey tile. ) else ( echo FAILED - copy this window's text and send it. )
pause
exit /b %RC2%
