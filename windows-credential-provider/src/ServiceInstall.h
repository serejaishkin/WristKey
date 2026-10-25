#pragma once

#include <windows.h>
#include <string>

bool ResolveWristKeyExePath(HMODULE hModule, std::wstring& exePathOut);
void EnsureWristKeyServiceRunning(const std::wstring& exePath);
void RemoveWristKeyService();
