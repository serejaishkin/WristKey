#include "ServiceInstall.h"
#include <windows.h>
#include <string>
#include <vector>
#include <shlwapi.h>

#pragma comment(lib, "shlwapi.lib")
#pragma comment(lib, "advapi32.lib")

bool ResolveWristKeyExePath(HMODULE hModule, std::wstring& exePathOut) {
    wchar_t path[MAX_PATH] = {0};
    if (GetModuleFileNameW(hModule, path, ARRAYSIZE(path)) == 0) {
        return false;
    }
    exePathOut = path;
    return true;
}

void EnsureWristKeyServiceRunning(const std::wstring& exePath) {
    // Best-effort: try to start the WristKey service
    SC_HANDLE scm = OpenSCManagerW(nullptr, nullptr, SC_MANAGER_CONNECT);
    if (!scm) return;

    SC_HANDLE svc = OpenSCManagerW(nullptr, nullptr, SC_MANAGER_CONNECT);
    if (!svc) {
        CloseServiceHandle(scm);
        return;
    }

    // Try to open existing service
    SC_HANDLE hSvc = OpenServiceW(scm, L"WristKey", SERVICE_QUERY_STATUS | SERVICE_START);
    if (hSvc) {
        SERVICE_STATUS_PROCESS ssp = {0};
        DWORD dwBytes = 0;
        if (QueryServiceStatusEx(hSvc, SC_STATUS_PROCESS_INFO, (LPBYTE)&ssp, sizeof(ssp), &dwBytes)) {
            if (ssp.dwCurrentState == SERVICE_STOPPED) {
                StartServiceW(hSvc, 0, nullptr);
            }
        }
        CloseServiceHandle(hSvc);
    }
    CloseServiceHandle(scm);
}

void RemoveWristKeyService() {
    SC_HANDLE scm = OpenSCManagerW(nullptr, nullptr, SC_MANAGER_CONNECT);
    if (!scm) return;

    SC_HANDLE hSvc = OpenServiceW(scm, L"WristKey", SERVICE_STOP | DELETE);
    if (hSvc) {
        SERVICE_STATUS status = {0};
        ControlService(hSvc, SERVICE_CONTROL_STOP, &status);
        DeleteService(hSvc);
        CloseServiceHandle(hSvc);
    }
    CloseServiceHandle(scm);
}
