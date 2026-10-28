#pragma once

#include <windows.h>
#include <string>

// Reads HKLM\SOFTWARE\WristKey\ExePath (written by the app on registration);
// falls back to WristKey.exe next to / one level above the DLL.
bool ResolveWristKeyExePath(HMODULE hModule, std::wstring& exePathOut);

// Makes sure the "WristKey" service exists (creates it from `exePath` when
// missing, repoints it when the binary moved) and is running. Best-effort,
// safe to call repeatedly. `exePath` empty = resolve from registry/DLL location.
void EnsureWristKeyServiceRunning(const std::wstring& exePath);

void RemoveWristKeyService();
