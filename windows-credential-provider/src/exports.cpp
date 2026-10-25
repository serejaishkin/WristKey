#include "Provider.h"
#include "ServiceInstall.h"
#include <shlwapi.h>
#include <new>
#include <string>

extern "C" const GUID CLSID_WristKeyCredentialProvider;

static HMODULE g_module = nullptr;
static LONG g_objects = 0;
static LONG g_locks = 0;

class ClassFactory final : public IClassFactory {
public:
    explicit ClassFactory() : ref_(1) { InterlockedIncrement(&g_objects); }
    ~ClassFactory() { InterlockedDecrement(&g_objects); }

    STDMETHOD(QueryInterface)(REFIID riid, void** ppv) override {
        if (!ppv) return E_POINTER; *ppv = nullptr;
        if (riid == IID_IUnknown || riid == IID_IClassFactory) {
            *ppv = static_cast<IClassFactory*>(this);
            AddRef();
            return S_OK;
        }
        return E_NOINTERFACE;
    }
    STDMETHOD_(ULONG, AddRef)() override { return InterlockedIncrement(&ref_); }
    STDMETHOD_(ULONG, Release)() override {
        ULONG n = InterlockedDecrement(&ref_);
        if (!n) delete this;
        return n;
    }

    STDMETHOD(CreateInstance)(IUnknown* outer, REFIID riid, void** ppv) override {
        if (outer) return CLASS_E_NOAGGREGATION;
        auto* p = new (std::nothrow) WristKeyProvider();
        if (!p) return E_OUTOFMEMORY;
        HRESULT hr = p->QueryInterface(riid, ppv);
        p->Release();
        return hr;
    }
    STDMETHOD(LockServer)(BOOL lock) override {
        if (lock) InterlockedIncrement(&g_locks);
        else InterlockedDecrement(&g_locks);
        return S_OK;
    }

private:
    LONG ref_;
};

BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) {
        g_module = h;
        DisableThreadLibraryCalls(h);
    }
    return TRUE;
}

// -----------------------------------------------------------------------------
// Registry helpers — remove ALL WristKey registrations from a single hive root
// -----------------------------------------------------------------------------

static void RemoveWristKeyRegistrationsFromRoot(HKEY root) {
    wchar_t key[MAX_PATH]{};
    wchar_t clsid[64]{};
    StringFromGUID2(CLSID_WristKeyCredentialProvider, clsid, ARRAYSIZE(clsid));

    // Credential Providers registration point
    swprintf_s(key, L"Software\\Microsoft\\Windows\\CurrentVersion\\Authentication\\Credential Providers\\%s", clsid);
    RegDeleteTreeW(root, key);

    // COM class registration
    swprintf_s(key, L"Software\\Classes\\CLSID\\%s", clsid);
    RegDeleteTreeW(root, key);

    // Legacy .NET provider CLSID
    swprintf_s(key, L"Software\\Microsoft\\Windows\\CurrentVersion\\Authentication\\Credential Providers\\%s", L"{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}");
    RegDeleteTreeW(root, key);
    swprintf_s(key, L"Software\\Classes\\CLSID\\%s", L"{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}");
    RegDeleteTreeW(root, key);

    // Microsoft sample control test CLSID
    swprintf_s(key, L"Software\\Microsoft\\Windows\\CurrentVersion\\Authentication\\Credential Providers\\%s", L"{5FD3D285-0DD9-4362-8855-E0ABAACD4AF6}");
    RegDeleteTreeW(root, key);
    swprintf_s(key, L"Software\\Classes\\CLSID\\%s", L"{5FD3D285-0DD9-4362-8855-E0ABAACD4AF6}");
    RegDeleteTreeW(root, key);
}

// Remove every WristKey registration from both HKLM and HKCU.
static void RemoveAllWristKeyRegistrations() {
    RemoveWristKeyRegistrationsFromRoot(HKEY_LOCAL_MACHINE);
    RemoveWristKeyRegistrationsFromRoot(HKEY_CURRENT_USER);
}

// -----------------------------------------------------------------------------
// DLL exports
// -----------------------------------------------------------------------------

extern "C"
HRESULT STDAPICALLTYPE DllGetClassObject(REFCLSID clsid, REFIID iid, void** ppv) {
    if (clsid != CLSID_WristKeyCredentialProvider) return CLASS_E_CLASSNOTAVAILABLE;
    auto* f = new (std::nothrow) ClassFactory();
    if (!f) return E_OUTOFMEMORY;
    HRESULT hr = f->QueryInterface(iid, ppv);
    f->Release();
    return hr;
}

extern "C"
HRESULT STDAPICALLTYPE DllCanUnloadNow() {
    return (g_objects == 0 && g_locks == 0) ? S_OK : S_FALSE;
}

static HRESULT WriteClsidKey(HKEY root, const wchar_t* subkey, const wchar_t* value) {
    HKEY key = nullptr;
    LONG rc = RegCreateKeyExW(root, subkey, 0, nullptr, REG_OPTION_NON_VOLATILE, KEY_WRITE, nullptr, &key, nullptr);
    if (rc != ERROR_SUCCESS) return HRESULT_FROM_WIN32(rc);
    rc = RegSetValueExW(key, nullptr, 0, REG_SZ,
                        reinterpret_cast<const BYTE*>(value),
                        static_cast<DWORD>((wcslen(value) + 1) * sizeof(wchar_t)));
    RegCloseKey(key);
    return HRESULT_FROM_WIN32(rc);
}

extern "C"
HRESULT STDAPICALLTYPE DllRegisterServer() {
    // ALWAYS clean first: remove every previous WristKey registration from
    // both HKLM and HKCU. This guarantees that repeated install/update never
    // leaves duplicate tiles on the sign-in screen.
    RemoveAllWristKeyRegistrations();

    wchar_t path[MAX_PATH]{};
    if (!GetModuleFileNameW(g_module, path, ARRAYSIZE(path)))
        return HRESULT_FROM_WIN32(GetLastError());

    wchar_t clsid[64]{};
    StringFromGUID2(CLSID_WristKeyCredentialProvider, clsid, ARRAYSIZE(clsid));

    // Register COM class
    wchar_t key[MAX_PATH]{};
    swprintf_s(key, L"Software\\Classes\\CLSID\\%s", clsid);
    HKEY hk = nullptr;
    LONG rc = RegCreateKeyExW(HKEY_LOCAL_MACHINE, key, 0, nullptr, REG_OPTION_NON_VOLATILE,
                              KEY_WRITE, nullptr, &hk, nullptr);
    if (rc != ERROR_SUCCESS) return HRESULT_FROM_WIN32(rc);
    RegSetValueExW(hk, nullptr, 0, REG_SZ,
                   reinterpret_cast<const BYTE*>(L"WristKey Credential Provider"),
                   static_cast<DWORD>((wcslen(L"WristKey Credential Provider") + 1) * sizeof(wchar_t)));
    RegCloseKey(hk);

    // InprocServer32
    swprintf_s(key, L"Software\\Classes\\CLSID\\%s\\InprocServer32", clsid);
    rc = RegCreateKeyExW(HKEY_LOCAL_MACHINE, key, 0, nullptr, REG_OPTION_NON_VOLATILE,
                         KEY_WRITE, nullptr, &hk, nullptr);
    if (rc != ERROR_SUCCESS) return HRESULT_FROM_WIN32(rc);
    RegSetValueExW(hk, nullptr, 0, REG_SZ,
                   reinterpret_cast<const BYTE*>(path),
                   static_cast<DWORD>((wcslen(path) + 1) * sizeof(wchar_t)));
    const wchar_t model[] = L"Apartment";
    RegSetValueExW(hk, L"ThreadingModel", 0, REG_SZ,
                   reinterpret_cast<const BYTE*>(model),
                   static_cast<DWORD>((wcslen(model) + 1) * sizeof(wchar_t)));
    RegCloseKey(hk);

    // Credential Providers registration point
    swprintf_s(key, L"Software\\Microsoft\\Windows\\CurrentVersion\\Authentication\\Credential Providers\\%s", clsid);
    rc = RegCreateKeyExW(HKEY_LOCAL_MACHINE, key, 0, nullptr, REG_OPTION_NON_VOLATILE,
                         KEY_WRITE, nullptr, &hk, nullptr);
    if (rc != ERROR_SUCCESS) return HRESULT_FROM_WIN32(rc);
    RegSetValueExW(hk, nullptr, 0, REG_SZ,
                   reinterpret_cast<const BYTE*>(L"WristKey Credential Provider"),
                   static_cast<DWORD>((wcslen(L"WristKey Credential Provider") + 1) * sizeof(wchar_t)));
    RegCloseKey(hk);

    // Best-effort: start the WristKey service so the daemon answers \\.\pipe\wristkey
    std::wstring exePath;
    if (ResolveWristKeyExePath(g_module, exePath)) {
        EnsureWristKeyServiceRunning(exePath);
    }

    return S_OK;
}

extern "C"
HRESULT STDAPICALLTYPE DllUnregisterServer() {
    RemoveAllWristKeyRegistrations();
    RemoveWristKeyService();
    return S_OK;
}
