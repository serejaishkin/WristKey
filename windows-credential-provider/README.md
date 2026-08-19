# WristKey Windows Credential Provider

This directory contains the first x64 Windows Credential Provider for WristKey.

## Current scope

This version is intentionally **UI-only**. It registers a real Windows Credential Provider and exposes a `WristKey` tile for `CPUS_LOGON` and `CPUS_UNLOCK_WORKSTATION`, but `GetSerialization()` does not yet submit a Windows credential. It returns `CPGSR_NO_CREDENTIAL_NOT_FINISHED` until the daemon IPC and Windows authentication serialization are wired.

Do not treat this build as a working passwordless Windows login yet.

## Build

Use a Visual Studio Developer PowerShell with the Windows 10/11 SDK:

```powershell
cmake -S windows-credential-provider -B windows-credential-provider/build -A x64
cmake --build windows-credential-provider/build --config Release
```

Output:

`windows-credential-provider/build/Release/WristKeyCredentialProvider.dll`

## Install

Run elevated 64-bit PowerShell:

```powershell
.\windows-credential-provider\install.ps1 -Action Install -DllPath .\windows-credential-provider\build\Release\WristKeyCredentialProvider.dll
```

Uninstall:

```powershell
.\windows-credential-provider\install.ps1 -Action Uninstall
```

Keep another working sign-in method enabled while testing Credential Providers. A broken provider can affect the Windows logon UI.

## Next step

Wire the tile to the WristKey daemon through the daemon's authenticated named-pipe protocol, then construct the correct Windows Credential Provider serialization for the target account. The DLL must not perform BLE itself.
