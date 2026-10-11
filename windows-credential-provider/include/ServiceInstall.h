#pragma once
#include <windows.h>
#include <string>

// Registers/starts (or removes) the WristKey background service directly
// from the Credential Provider DLL, so the daemon that answers
// \\.\pipe\wristkey is guaranteed to be running before any user signs in --
// independent of whether the WristKey GUI app was ever launched, whether it
// was launched elevated, or whether anyone has logged on this boot yet.
//
// DllRegisterServer (called by regsvr32, which is already elevated) is the
// single place this is driven from, so there is no separate opt-in step a
// user can forget.

// Finds the WristKey.exe that belongs to this install. Checks, in order:
//   1. HKLM\Software\WristKey\ExePath (REG_SZ) -- the explicit, authoritative
//      location an installer (or install.ps1) should set.
//   2. The directory this DLL itself lives in (WristKey.exe next to
//      WristKeyCredentialProvider.dll -- true when both are deployed from
//      the same install folder, e.g. by the NSIS/MSI bundle).
//   3. %ProgramFiles%\WristKey\WristKey.exe (the default NSIS/MSI install
//      location for the "WristKey" product).
// Returns true and fills outPath if found, false otherwise.
bool ResolveWristKeyExePath(HMODULE dllModule, std::wstring& outPath);

// Creates the "WristKey" service (SERVICE_AUTO_START, LocalSystem) pointing
// at `exePath --service` if it does not exist yet; if it exists, makes sure
// it is configured for auto-start and points at `exePath`. Starts it if not
// already running. Best-effort: logs via OutputDebugStringW and returns
// false on failure rather than throwing -- a failed service install must
// not fail Credential Provider COM registration, or the DLL stops loading
// in LogonUI entirely.
bool EnsureWristKeyServiceRunning(const std::wstring& exePath);

// Stops and deletes the "WristKey" service, if present. Best-effort.
void RemoveWristKeyService();
