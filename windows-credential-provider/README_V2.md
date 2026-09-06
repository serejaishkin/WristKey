# WristKey Windows Credential Provider V2

The native x64 provider is the production Windows sign-in path. The old `.NET 4.8` provider is legacy and is not used for the Windows 10/11 **Sign-in options** tile.

## Build on Windows

From the repository root in PowerShell:

```powershell
cmake -S windows-credential-provider -B windows-credential-provider/build -A x64
cmake --build windows-credential-provider/build --config Release
```

The DLL is:

```text
windows-credential-provider\build\Release\WristKeyCredentialProvider.dll
```

## Install

Run PowerShell **as Administrator**:

```powershell
powershell -ExecutionPolicy Bypass -File .\windows-credential-provider\install.ps1 `
  -Action Install `
  -DllPath "$PWD\windows-credential-provider\build\Release\WristKeyCredentialProvider.dll"
```

Then **sign out/in or reboot Windows**. Open **Sign-in options** on the Windows lock screen. WristKey is implemented as a V2 Credential Provider and should appear as its own WristKey option/icon.

The installer also removes the legacy managed-provider registration so the two implementations cannot compete for the same login UI.

## Uninstall

```powershell
powershell -ExecutionPolicy Bypass -File .\windows-credential-provider\install.ps1 -Action Uninstall
```

## CI artifact

The `Build All Artifacts` workflow now builds the native x64 provider and publishes:

```text
wristkey-windows-credential-provider-x64
```

The native provider is registered through its exported `DllRegisterServer`; Windows uses the native DLL directly as the COM `InprocServer32` implementation.
