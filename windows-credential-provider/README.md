# WristKey Windows Credential Provider

This directory contains the first native x64 Windows Credential Provider for WristKey.

## Current scope

This version is intentionally **UI-only**. It registers a real Windows Credential Provider and exposes a `WristKey` tile for `CPUS_LOGON` and `CPUS_UNLOCK_WORKSTATION`, but `GetSerialization()` does not yet submit a Windows credential. It returns `CPGSR_NO_CREDENTIAL_NOT_FINISHED` until daemon IPC and Windows authentication serialization are wired.

The provider enumerates enabled local Windows accounts and creates one WristKey credential tile per account. Passwords are not stored by WristKey.

Do not treat this build as a working passwordless Windows login yet.

## Build with Visual Studio — recommended

No CMake installation is required for the normal Windows developer workflow.

Open:

`windows-credential-provider/WristKeyCredentialProvider.sln`

Then select:

- **Release**
- **x64**

and use **Build → Build Solution**.

The project uses the native MSVC v143 toolset and the Windows SDK. The resulting DLL is placed under:

`windows-credential-provider/build/Release/WristKeyCredentialProvider.dll`

A CMake build is still supported for environments that already have CMake installed.

## Install

Run elevated 64-bit PowerShell:

```powershell
.\windows-credential-provider\install.ps1 -Action Install -DllPath .\windows-credential-provider\build\Release\WristKeyCredentialProvider.dll
```

Uninstall:

```powershell
.\windows-credential-provider\install.ps1 -Action Uninstall
```

Keep another working sign-in method enabled while testing Credential Providers. A broken provider runs inside the Windows logon UI and can affect sign-in.

## Architecture direction

The DLL is intentionally a thin Windows LogonUI integration layer. BLE, pairing, cryptography and device management belong to the WristKey daemon. The intended flow is:

```text
Windows LogonUI
      ↓
WristKeyCredentialProvider.dll
      ↓ authenticated local IPC
WristKey daemon
      ↓
BLE / crypto
      ↓
Galaxy Watch
```

For multi-user support, a watch identity/public key will be associated with a Windows account during enrollment. The provider should never become a password vault.

## Reference project

The project takes architectural inspiration from **PC Bio Unlock Desktop** (`MeisApps/pcbu-desktop`): a desktop service handles the device connection while the Windows Credential Provider remains the thin LogonUI integration point. PC Bio Unlock supports Windows login/unlock and UAC scenarios and uses TCP/Bluetooth for the phone connection. WristKey deliberately keeps its own BLE/cryptographic protocol and does not copy or depend on PC Bio Unlock code.

## Next step

Wire the tile to the WristKey daemon through an authenticated named-pipe protocol, map the authenticated watch public key to the selected Windows account, and construct the correct Windows Credential Provider serialization for that account. The DLL must not perform BLE itself.
