@echo off
rem WristKey MSA-server build wrapper.
rem Loads MSVC x64 env (needed for msvcrt.lib), then builds + tests the server crate.
setlocal
set ROOT=%~dp0
set VSWHERE="%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
for /f "usebackq delims=" %%i in (`%VSWHERE% -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set VSINST=%%i
if not defined VSINST (
  echo can't locate VS installation & exit /b 1
)
call "%VSINST%\VC\Auxiliary\Build\vcvars64.bat" >nul
if errorlevel 1 ( echo vcvars64 failed & exit /b 1 )
cd /d "%ROOT%"
set MODE=%~1
if "%MODE%"=="" set MODE=build
if "%MODE%"=="test" (
  cargo test --release -p wristkey-msa 2>&1
) else (
  cargo build --release -p wristkey-msa 2>&1
)
exit /b %errorlevel%