#include "ServiceInstall.h"
#include <shlwapi.h>
#include <shlobj.h>
#include <string>

#pragma comment(lib, "advapi32.lib")
#pragma comment(lib, "shlwapi.lib")
#pragma comment(lib, "shell32.lib")

namespace {

const wchar_t* kServiceName = L"WristKey";
const wchar_t* kDisplayName = L"WristKey BLE Unlock";

void LogDebug(const wchar_t* message) {
    OutputDebugStringW(L"[WristKeyCredentialProvider] ");
    OutputDebugStringW(message);
    OutputDebugStringW(L"\n");
}

bool FileExists(const std::wstring& path) {
    const DWORD attrs = GetFileAttributesW(path.c_str());
    return attrs != INVALID_FILE_ATTRIBUTES && !(attrs & FILE_ATTRIBUTE_DIRECTORY);
}

bool ReadExePathFromRegistry(std::wstring& outPath) {
    HKEY key = nullptr;
    if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, L"Software\\WristKey", 0, KEY_READ, &key) != ERROR_SUCCESS) {
        return false;
    }

    wchar_t buffer[MAX_PATH]{};
    DWORD size = sizeof(buffer);
    DWORD type = 0;
    const LONG rc = RegQueryValueExW(key, L"ExePath", nullptr, &type,
                                      reinterpret_cast<BYTE*>(buffer), &size);
    RegCloseKey(key);

    if (rc != ERROR_SUCCESS || type != REG_SZ) return false;
    if (!FileExists(buffer)) return false;

    outPath = buffer;
    return true;
}

}  // namespace

bool ResolveWristKeyExePath(HMODULE dllModule, std::wstring& outPath) {
    // 1. Explicit, authoritative location (set by install.ps1 or the
    //    NSIS/MSI installer).
    if (ReadExePathFromRegistry(outPath)) return true;

    // 2. Sibling of this DLL -- true when the DLL is registered in place
    //    inside the WristKey install folder, next to WristKey.exe.
    wchar_t dllPath[MAX_PATH]{};
    if (GetModuleFileNameW(dllModule, dllPath, ARRAYSIZE(dllPath))) {
        PathRemoveFileSpecW(dllPath);
        std::wstring candidate = std::wstring(dllPath) + L"\\WristKey.exe";
        if (FileExists(candidate)) {
            outPath = candidate;
            return true;
        }
    }

    // 3. Default NSIS/MSI install location.
    wchar_t programFiles[MAX_PATH]{};
    if (SHGetFolderPathW(nullptr, CSIDL_PROGRAM_FILES, nullptr, 0, programFiles) == S_OK) {
        std::wstring candidate = std::wstring(programFiles) + L"\\WristKey\\WristKey.exe";
        if (FileExists(candidate)) {
            outPath = candidate;
            return true;
        }
    }

    return false;
}

bool EnsureWristKeyServiceRunning(const std::wstring& exePath) {
    if (exePath.empty()) {
        LogDebug(L"no WristKey.exe path resolved; skipping service install.");
        return false;
    }

    SC_HANDLE scm = OpenSCManagerW(nullptr, nullptr, SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE);
    if (!scm) {
        LogDebug(L"OpenSCManagerW failed; cannot install/start WristKey service.");
        return false;
    }

    const std::wstring binPath = L"\"" + exePath + L"\" --service";

    SC_HANDLE svc = OpenServiceW(scm, kServiceName,
                                  SERVICE_QUERY_STATUS | SERVICE_START | SERVICE_CHANGE_CONFIG);

    if (!svc) {
        svc = CreateServiceW(
            scm, kServiceName, kDisplayName,
            SERVICE_CHANGE_CONFIG | SERVICE_START | SERVICE_QUERY_STATUS,
            SERVICE_WIN32_OWN_PROCESS, SERVICE_AUTO_START, SERVICE_ERROR_NORMAL,
            binPath.c_str(), nullptr, nullptr, nullptr, nullptr, nullptr);
        if (!svc) {
            LogDebug(L"CreateServiceW failed.");
            CloseServiceHandle(scm);
            return false;
        }

        SERVICE_DESCRIPTIONW desc{};
        desc.lpDescription = const_cast<LPWSTR>(L"WristKey BLE PC Unlock Service");
        ChangeServiceConfig2W(svc, SERVICE_CONFIG_DESCRIPTION, &desc);
    } else {
        // Service already exists (from a previous install or from the app's
        // own self-install): make sure it is auto-start and points at the
        // current exe path, in case the install moved.
        ChangeServiceConfigW(svc, SERVICE_NO_CHANGE, SERVICE_AUTO_START, SERVICE_NO_CHANGE,
                              binPath.c_str(), nullptr, nullptr, nullptr, nullptr, nullptr, kDisplayName);
    }

    SERVICE_STATUS_PROCESS statusBuf{};
    DWORD bytesNeeded = 0;
    bool running = false;
    if (QueryServiceStatusEx(svc, SC_STATUS_PROCESS_INFO, reinterpret_cast<BYTE*>(&statusBuf),
                              sizeof(statusBuf), &bytesNeeded)) {
        running = statusBuf.dwCurrentState == SERVICE_RUNNING ||
                  statusBuf.dwCurrentState == SERVICE_START_PENDING;
    }

    bool ok = running;
    if (!running) {
        ok = StartServiceW(svc, 0, nullptr) != FALSE;
        if (!ok) {
            const DWORD err = GetLastError();
            if (err == ERROR_SERVICE_ALREADY_RUNNING) {
                ok = true;
            } else {
                LogDebug(L"StartServiceW failed.");
            }
        }
    }

    CloseServiceHandle(svc);
    CloseServiceHandle(scm);
    return ok;
}

void RemoveWristKeyService() {
    SC_HANDLE scm = OpenSCManagerW(nullptr, nullptr, SC_MANAGER_CONNECT);
    if (!scm) return;

    SC_HANDLE svc = OpenServiceW(scm, kServiceName,
                                  SERVICE_QUERY_STATUS | SERVICE_STOP | DELETE);
    if (svc) {
        SERVICE_STATUS status{};
        ControlService(svc, SERVICE_CONTROL_STOP, &status);
        // Give it a moment to stop; deleting a running service only marks it
        // for deletion on next stop, which is fine either way.
        for (int i = 0; i < 20; ++i) {
            SERVICE_STATUS_PROCESS statusBuf{};
            DWORD bytesNeeded = 0;
            if (QueryServiceStatusEx(svc, SC_STATUS_PROCESS_INFO, reinterpret_cast<BYTE*>(&statusBuf),
                                      sizeof(statusBuf), &bytesNeeded) &&
                statusBuf.dwCurrentState == SERVICE_STOPPED) {
                break;
            }
            Sleep(100);
        }
        DeleteService(svc);
        CloseServiceHandle(svc);
    }

    CloseServiceHandle(scm);
}
