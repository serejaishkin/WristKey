@echo off
rem Build the MSI/NSIS installer via cargo tauri build.
rem Uses VS2022 BuildTools to avoid VS18 Community (broken).
set "VS=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207"
set "SDK=C:\Program Files (x86)\Windows Kits\10"
set "SDKVER=10.0.26100.0"
set "INCLUDE=%VS%\include;%SDK%\Include\%SDKVER%\ucrt;%SDK%\Include\%SDKVER%\um;%SDK%\Include\%SDKVER%\shared"
set "LIB=%VS%\lib\x64;%SDK%\Lib\%SDKVER%\ucrt\x64;%SDK%\Lib\%SDKVER%\um\x64"
set "PATH=%VS%\bin\Hostx64\x64;%PATH%"
rem Bypass cc-rs vswhere detection (picks broken VS18 otherwise)
set "CC_x86_64_pc_windows_msvc=%VS%\bin\Hostx64\x64\cl.exe"
set "CXX_x86_64_pc_windows_msvc=%VS%\bin\Hostx64\x64\cl.exe"
cd /d D:\GitHub\WristKey\desktop
cargo tauri build %*
