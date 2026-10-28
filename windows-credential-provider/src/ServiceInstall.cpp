#include "ServiceInstall.h"

#include <windows.h>
#include <shlwapi.h>

#include <string>

#ifdef _MSC_VER
#pragma comment(lib, "shlwapi.lib")
#pragma comment(lib, "advapi32.lib")
#endif

static const wchar_t kServiceName[] = L"WristKey";

static bool ReadRegString(const wchar_t* name, std::wstring& out) {
    HKEY key = nullptr;
    if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, L"SOFTWARE\\WristKey", 0, KEY_READ | KEY_WOW64_64KEY, &key) != ERROR_SUCCESS)
        return false;
    wchar_t buf[1024]{};
    DWORD size = sizeof(buf) - sizeof(wchar_t);
    DWORD type = 0;
    const LONG rc = RegQueryValueExW(key, name, nullptr, &type, reinterpret_cast<LPBYTE>(buf), &size);
    RegCloseKey(key);
    if (rc != ERROR_SUCCESS || type != REG_SZ || !buf[0]) return false;
    out = buf;
    return true;
}

bool ResolveWristKeyExePath(HMODULE hModule, std::wstring& exePathOut) {
    std::wstring p;
    if (ReadRegString(L"ExePath", p) && PathFileExistsW(p.c_str())) {
        exePathOut = p;
        return true;
    }
    wchar_t dll[MAX_PATH]{};
    if (!hModule || !GetModuleFileNameW(hModule, dll, ARRAYSIZE(dll))) return false;
    PathRemoveFileSpecW(dll);
    for (int up = 0; up < 2; ++up) {
        wchar_t cand[MAX_PATH]{};
        PathCombineW(cand, dll, L"WristKey.exe");
        if (PathFileExistsW(cand)) { exePathOut = cand; return true; }
        PathRemoveFileSpecW(dll);
    }
    return false;
}

static std::wstring BuildCommandLine(const std::wstring& exe) {
    std::wstring cmd = L"\"" + exe + L"\" --service";
    std::wstring dataDir;
    if (ReadRegString(L"DataDir", dataDir)) cmd += L" --data-dir \"" + dataDir + L"\"";
    return cmd;
}

void EnsureWristKeyServiceRunning(const std::wstring& exePathIn) {
    std::wstring exe = exePathIn;
    if (exe.empty()) {
        HMODULE self = nullptr;
        GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                           reinterpret_cast<LPCWSTR>(&EnsureWristKeyServiceRunning), &self);
        if (!ResolveWristKeyExePath(self, exe)) exe.clear();
    }

    SC_HANDLE scm = OpenSCManagerW(nullptr, nullptr, SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE);
    if (!scm) return;

    SC_HANDLE svc = OpenServiceW(scm, kServiceName,
                                 SERVICE_QUERY_STATUS | SERVICE_START | SERVICE_CHANGE_CONFIG | SERVICE_QUERY_CONFIG);
    if (!svc && GetLastError() == ERROR_SERVICE_DOES_NOT_EXIST && !exe.empty() && PathFileExistsW(exe.c_str())) {
        const std::wstring cmd = BuildCommandLine(exe);
        svc = CreateServiceW(scm, kServiceName, L"WristKey BLE Unlock",
                             SERVICE_ALL_ACCESS, SERVICE_WIN32_OWN_PROCESS, SERVICE_AUTO_START,
                             SERVICE_ERROR_NORMAL, cmd.c_str(), nullptr, nullptr, nullptr, nullptr, nullptr);
    } else if (svc && !exe.empty()) {
        // Repoint a service whose binary moved (e.g. reinstall into another folder).
        DWORD need = 0;
        QueryServiceConfigW(svc, nullptr, 0, &need);
        if (need) {
            std::string buf(need, '\0');
            auto* cfg = reinterpret_cast<QUERY_SERVICE_CONFIGW*>(&buf[0]);
            if (QueryServiceConfigW(svc, cfg, need, &need)) {
                const std::wstring want = BuildCommandLine(exe);
                if (!cfg->lpBinaryPathName || _wcsicmp(cfg->lpBinaryPathName, want.c_str()) != 0) {
                    ChangeServiceConfigW(svc, SERVICE_NO_CHANGE, SERVICE_AUTO_START, SERVICE_NO_CHANGE,
                                         want.c_str(), nullptr, nullptr, nullptr, nullptr, nullptr, nullptr);
                }
            }
        }
    }

    if (svc) {
        SERVICE_STATUS_PROCESS ssp{};
        DWORD bytes = 0;
        if (QueryServiceStatusEx(svc, SC_STATUS_PROCESS_INFO, reinterpret_cast<LPBYTE>(&ssp), sizeof(ssp), &bytes) &&
            ssp.dwCurrentState == SERVICE_STOPPED) {
            StartServiceW(svc, 0, nullptr);
        }
        CloseServiceHandle(svc);
    }
    CloseServiceHandle(scm);
}

void RemoveWristKeyService() {
    SC_HANDLE scm = OpenSCManagerW(nullptr, nullptr, SC_MANAGER_CONNECT);
    if (!scm) return;
    SC_HANDLE svc = OpenServiceW(scm, kServiceName, SERVICE_STOP | DELETE);
    if (svc) {
        SERVICE_STATUS status{};
        ControlService(svc, SERVICE_CONTROL_STOP, &status);
        DeleteService(svc);
        CloseServiceHandle(svc);
    }
    CloseServiceHandle(scm);
}
