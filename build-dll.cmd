@echo off
rem ============================================================================
rem  build-dll.cmd -- builds the WristKey Credential Provider DLL (x64, Release)
rem
rem  Usage (from the repo root):
rem      build-dll.cmd            configure + build
rem      build-dll.cmd clean      wipe windows-credential-provider\build first
rem      build-dll.cmd test       also run the safe smoke tests afterwards
rem      build-dll.cmd clean test both
rem
rem  From Git Bash:  cmd //c build-dll.cmd [clean] [test]
rem
rem  Toolchain: uses an already-initialised developer prompt if `cl` is on PATH,
rem  otherwise looks for VS2022 BuildTools, then any VS found by vswhere, then
rem  falls back to composing INCLUDE/LIB/PATH by hand (the VS18 Community
rem  install on this machine has a broken vcvars, see build-provider.cmd).
rem
rem  Output: windows-credential-provider\build\Release\WristKeyCredentialProvider.dll
rem  Exit code is 0 only if the DLL was produced (and, with `test`, tests passed).
rem ============================================================================
setlocal EnableExtensions

set "ROOT=%~dp0"
set "SRC=%ROOT%windows-credential-provider"
set "BLD=%SRC%\build"
set "DLL=%BLD%\Release\WristKeyCredentialProvider.dll"
set "PF86=%ProgramFiles(x86)%"
set "DO_CLEAN=0"
set "DO_TEST=0"

:args
if "%~1"=="" goto args_done
if /i "%~1"=="clean" set "DO_CLEAN=1"
if /i "%~1"=="test" set "DO_TEST=1"
shift
goto args
:args_done

if not exist "%SRC%\CMakeLists.txt" (
    echo [build-dll] ERROR: %SRC%\CMakeLists.txt not found. Run this from the repo root.
    exit /b 2
)

rem ---- 1. compiler -----------------------------------------------------------
where cl >nul 2>&1
if not errorlevel 1 goto have_cl

set "VCVARS=%PF86%\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if exist "%VCVARS%" call "%VCVARS%" >nul 2>&1
where cl >nul 2>&1
if not errorlevel 1 goto have_cl

set "VSWHERE=%PF86%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" goto manual_env
for /f "usebackq delims=" %%I in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSROOT=%%I"
if not defined VSROOT goto manual_env
if exist "%VSROOT%\VC\Auxiliary\Build\vcvars64.bat" call "%VSROOT%\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1
where cl >nul 2>&1
if not errorlevel 1 goto have_cl

:manual_env
echo [build-dll] vcvars not usable, composing the environment by hand...
set "BT=%PF86%\Microsoft Visual Studio\2022\BuildTools"
set "MSVCVER="
for /d %%V in ("%BT%\VC\Tools\MSVC\*") do set "MSVCVER=%%~nxV"
if not defined MSVCVER (
    echo [build-dll] ERROR: no MSVC toolset under %BT%\VC\Tools\MSVC
    echo             Install "Desktop development with C++" ^(VS2022 Build Tools^).
    exit /b 3
)
set "VS=%BT%\VC\Tools\MSVC\%MSVCVER%"
set "SDK=%PF86%\Windows Kits\10"
set "SDKVER="
for /d %%V in ("%SDK%\Include\10.*") do set "SDKVER=%%~nxV"
if not defined SDKVER (
    echo [build-dll] ERROR: no Windows 10/11 SDK under %SDK%\Include
    exit /b 3
)
set "INCLUDE=%VS%\include;%SDK%\Include\%SDKVER%\ucrt;%SDK%\Include\%SDKVER%\um;%SDK%\Include\%SDKVER%\shared"
set "LIB=%VS%\lib\x64;%SDK%\Lib\%SDKVER%\ucrt\x64;%SDK%\Lib\%SDKVER%\um\x64"
set "PATH=%VS%\bin\Hostx64\x64;%SDK%\bin\%SDKVER%\x64;%PATH%"
where cl >nul 2>&1
if errorlevel 1 (
    echo [build-dll] ERROR: cl.exe still not found after setting up the environment.
    exit /b 3
)

:have_cl
where cmake >nul 2>&1
if errorlevel 1 (
    echo [build-dll] ERROR: cmake not found in PATH ^(>= 3.20 required^).
    exit /b 4
)
echo [build-dll] compiler:
where cl
echo.

rem ---- 2. clean --------------------------------------------------------------
if "%DO_CLEAN%"=="1" (
    echo [build-dll] cleaning %BLD%
    if exist "%BLD%" rmdir /s /q "%BLD%"
)

rem The DLL may be locked if it is loaded by a running LogonUI/regsvr32 test.
if exist "%DLL%" (
    del /q "%DLL%" >nul 2>&1
    if exist "%DLL%" (
        echo [build-dll] ERROR: cannot overwrite %DLL% -- it is in use.
        echo             Stop WristKey / close anything that loaded it and retry.
        exit /b 5
    )
)

rem ---- 3. configure ----------------------------------------------------------
rem Prefer the VS2022 generator; if that fails (different VS, stale cache from
rem another generator) wipe the cache and let CMake pick its default.
cmake -S "%SRC%" -B "%BLD%" -G "Visual Studio 17 2022" -A x64
if errorlevel 1 (
    echo [build-dll] VS2022 generator failed, retrying with CMake's default generator...
    if exist "%BLD%\CMakeCache.txt" del /q "%BLD%\CMakeCache.txt"
    if exist "%BLD%\CMakeFiles" rmdir /s /q "%BLD%\CMakeFiles"
    cmake -S "%SRC%" -B "%BLD%" -A x64
    if errorlevel 1 (
        echo [build-dll] ERROR: cmake configure failed.
        exit /b 6
    )
)

rem ---- 4. build --------------------------------------------------------------
cmake --build "%BLD%" --config Release --parallel
if errorlevel 1 (
    echo [build-dll] ERROR: build failed.
    exit /b 7
)

if not exist "%DLL%" (
    echo [build-dll] ERROR: build reported success but %DLL% is missing.
    exit /b 8
)

echo.
echo [build-dll] OK: %DLL%
for %%F in ("%DLL%") do echo [build-dll] size: %%~zF bytes, modified %%~tF

rem ---- 5. optional smoke tests -----------------------------------------------
rem Deliberately NOT RegistrationSmokeTest: it calls DllRegisterServer /
rem DllUnregisterServer and would touch the installed provider.
if not "%DO_TEST%"=="1" goto done
set "FAILED=0"
for %%T in (ProviderSmokeTest ScenarioSafetySmokeTest UserArraySmokeTest SerializationSmokeTest) do call :run_test %%T
if not "%FAILED%"=="0" (
    echo [build-dll] %FAILED% smoke test^(s^) FAILED.
    exit /b 9
)
echo [build-dll] all smoke tests passed.

:done
echo.
echo Install ^(Administrator PowerShell^):
echo   powershell -ExecutionPolicy Bypass -File "%SRC%\install.ps1" -Action Install -DllPath "%DLL%"
exit /b 0

:run_test
set "EXE=%BLD%\Release\%1.exe"
if not exist "%EXE%" (
    echo [build-dll] %1: exe not found, skipped
    exit /b 0
)
"%EXE%" >nul 2>&1
if errorlevel 1 (
    echo [build-dll] %1: FAILED
    set /a FAILED+=1
) else (
    echo [build-dll] %1: ok
)
exit /b 0
