@echo off
rem VS18 Community has an empty lib dir and a broken vcvars (missing
rem vcvarsall.bat); use the VS2022 BuildTools toolchain instead.
set "VS=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207"
set "SDK=C:\Program Files (x86)\Windows Kits\10"
set "SDKVER=10.0.26100.0"
set "INCLUDE=%VS%\include;%SDK%\Include\%SDKVER%\ucrt;%SDK%\Include\%SDKVER%\um;%SDK%\Include\%SDKVER%\shared"
set "LIB=%VS%\lib\x64;%SDK%\Lib\%SDKVER%\ucrt\x64;%SDK%\Lib\%SDKVER%\um\x64"
set "PATH=%VS%\bin\Hostx64\x64;%PATH%"
cd /d D:\GitHub\WristKey
cmake -S windows-credential-provider -B windows-credential-provider\build -A x64 && cmake --build windows-credential-provider\build --config Release
