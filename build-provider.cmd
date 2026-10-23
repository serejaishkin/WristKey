@echo off
setlocal
rem VS18 Community has an empty lib dir and a broken vcvars (missing
rem vcvarsall.bat); use the VS2022 BuildTools toolchain instead.
set "VS=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207"
set "SDK=C:\Program Files (x86)\Windows Kits\10"
set "SDKVER=10.0.26100.0"
set "INCLUDE=%VS%\include;%SDK%\Include\%SDKVER%\ucrt;%SDK%\Include\%SDKVER%\um;%SDK%\Include\%SDKVER%\shared"
set "LIB=%VS%\lib\x64;%SDK%\Lib\%SDKVER%\ucrt\x64;%SDK%\Lib\%SDKVER%\um\x64"
set "PATH=%VS%\bin\Hostx64\x64;%PATH%"
set "ROOT=%~dp0"
set "BUILD=%ROOT%windows-credential-provider\build-vs2022"
cmake -S "%ROOT%windows-credential-provider" -B "%BUILD%" -G "Visual Studio 17 2022" -A x64
if errorlevel 1 exit /b %errorlevel%
cmake --build "%BUILD%" --config Release
if errorlevel 1 exit /b %errorlevel%
if not exist "%ROOT%windows-credential-provider\build\Release" mkdir "%ROOT%windows-credential-provider\build\Release"
copy /Y "%BUILD%\Release\WristKeyCredentialProvider.dll" "%ROOT%windows-credential-provider\build\Release\WristKeyCredentialProvider.dll"
if errorlevel 1 exit /b %errorlevel%
exit /b 0
