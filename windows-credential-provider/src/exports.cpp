#include "Provider.h"
#include "ServiceInstall.h"
#include <shlwapi.h>
#include <new>
#include <string>

static HMODULE g_module = nullptr;
static LONG g_objects = 0;
static LONG g_locks = 0;

class ClassFactory final : public IClassFactory {
public:
    explicit ClassFactory() : ref_(1) { InterlockedIncrement(&g_objects); }
    ~ClassFactory() { InterlockedDecrement(&g_objects); }
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void** ppv) override {
        if (!ppv) return E_POINTER; *ppv = nullptr;
        if (riid == IID_IUnknown || riid == IID_IClassFactory) { *ppv = static_cast<IClassFactory*>(this); AddRef(); return S_OK; }
        return E_NOINTERFACE;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return InterlockedIncrement(&ref_); }
    ULONG STDMETHODCALLTYPE Release() override { ULONG n = InterlockedDecrement(&ref_); if (!n) delete this; return n; }
    HRESULT STDMETHODCALLTYPE CreateInstance(IUnknown* outer, REFIID riid, void** ppv) override {
        if (outer) return CLASS_E_NOAGGREGATION;
        auto* p = new (std::nothrow) WristKeyProvider();
        if (!p) return E_OUTOFMEMORY;
        HRESULT hr = p->QueryInterface(riid, ppv);
        p->Release();
        return hr;
    }
    HRESULT STDMETHODCALLTYPE LockServer(BOOL lock) override {
        if (lock) InterlockedIncrement(&g_locks); else InterlockedDecrement(&g_locks);
        return S_OK;
    }
private: LONG ref_;
};

BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) { g_module = h; DisableThreadLibraryCalls(h); }
    return TRUE;
}

// Exports are declared in WristKeyCredentialProvider.def, matching Microsoft's
// Credential Provider sample. This avoids conflicting dllimport/dllexport
// declarations from the Windows COM headers for the standard COM entry points.
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
    rc = RegSetValueExW(key, nullptr, 0, REG_SZ, reinterpret_cast<const BYTE*>(value), static_cast<DWORD>((wcslen(value)+1)*sizeof(wchar_t)));
    RegCloseKey(key);
    return HRESULT_FROM_WIN32(rc);
}

// Registration entries left behind by provider implementations this project
// has superseded. If any of these are still registered they show up as
// EXTRA tiles on the sign-in screen next to WristKey's own tile and look
// like "duplicates" -- this has bitten the project before (the legacy .NET
// provider, and a Microsoft sample used as a one-off architecture control
// test). Cleaning them up here, inside DllRegisterServer itself, means the
// cleanup always happens on every install/update, regardless of which
// script or installer called regsvr32.
namespace {
struct LegacyClsid { const wchar_t* clsid; const wchar_t* note; };
constexpr LegacyClsid kLegacyClsids[] = {
    // Old managed (.NET/C#) WristKey Credential Provider. CLR-hosted COM
    // objects are unreliable inside LogonUI's secure desktop; this project
    // moved to the native C++ provider in this same DLL.
    { L"{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}", L"legacy managed provider" },
    // Microsoft's own, unmodified SampleV2CredentialProvider, registered by
    // test-microsoft-v2.ps1 as a control test ("does Windows accept this
    // architecture at all"). Never meant to stay registered permanently.
    { L"{5FD3D285-0DD9-4362-8855-E0ABAACD4AF6}", L"Microsoft sample control test" },
};

void RemoveClsidRegistration(const wchar_t* clsid) {
    wchar_t key[MAX_PATH]{};
    swprintf_s(key, L"Software\\Microsoft\\Windows\\CurrentVersion\\Authentication\\Credential Providers\\%s", clsid);
    RegDeleteTreeW(HKEY_LOCAL_MACHINE, key);
    swprintf_s(key, L"Software\\Classes\\CLSID\\%s", clsid);
    RegDeleteTreeW(HKEY_LOCAL_MACHINE, key);
}

void RemoveKnownDuplicateProviders() {
    for (const auto& entry : kLegacyClsids) {
        RemoveClsidRegistration(entry.clsid);
    }
}
}  // namespace

extern "C"
HRESULT STDAPICALLTYPE DllRegisterServer() {
    wchar_t path[MAX_PATH]{};
    if (!GetModuleFileNameW(g_module, path, ARRAYSIZE(path))) return HRESULT_FROM_WIN32(GetLastError());
    wchar_t clsid[64]{};
    StringFromGUID2(CLSID_WristKeyCredentialProvider, clsid, ARRAYSIZE(clsid));

    // Do this first: if a stale provider is still registered under a
    // different CLSID, remove it before adding our own tile, so there is
    // never a moment where both are present together.
    RemoveKnownDuplicateProviders();

    wchar_t key[MAX_PATH]{};
    swprintf_s(key, L"Software\\Classes\\CLSID\\%s", clsid);
    HRESULT hr = WriteClsidKey(HKEY_LOCAL_MACHINE, key, L"WristKey Credential Provider");
    if (FAILED(hr)) return hr;
    swprintf_s(key, L"Software\\Classes\\CLSID\\%s\\InprocServer32", clsid);
    hr = WriteClsidKey(HKEY_LOCAL_MACHINE, key, path);
    if (FAILED(hr)) return hr;
    HKEY server = nullptr;
    LONG rc = RegOpenKeyExW(HKEY_LOCAL_MACHINE, key, 0, KEY_WRITE, &server);
    if (rc == ERROR_SUCCESS) {
        const wchar_t model[] = L"Apartment";
        RegSetValueExW(server, L"ThreadingModel", 0, REG_SZ, reinterpret_cast<const BYTE*>(model), sizeof(model));
        RegCloseKey(server);
    }
    swprintf_s(key, L"Software\\Microsoft\\Windows\\CurrentVersion\\Authentication\\Credential Providers\\%s", clsid);
    hr = WriteClsidKey(HKEY_LOCAL_MACHINE, key, L"WristKey Credential Provider");
    if (FAILED(hr)) return hr;

    // The tile is useless without the daemon answering \\.\pipe\wristkey
    // BEFORE anyone signs in. regsvr32 already runs elevated, so this is
    // the one place that reliably has the rights to install/start the
    // service -- it does not depend on the GUI app ever being launched
    // (let alone launched elevated), and it does not depend on a separate
    // install script remembering to do it. Best-effort: a failure here
    // does not fail Credential Provider registration itself.
    std::wstring exePath;
    if (ResolveWristKeyExePath(g_module, exePath)) {
        EnsureWristKeyServiceRunning(exePath);
    }

    return S_OK;
}

extern "C"
HRESULT STDAPICALLTYPE DllUnregisterServer() {
    wchar_t clsid[64]{};
    StringFromGUID2(CLSID_WristKeyCredentialProvider, clsid, ARRAYSIZE(clsid));
    wchar_t key[MAX_PATH]{};
    swprintf_s(key, L"Software\\Classes\\CLSID\\%s\\InprocServer32", clsid); RegDeleteTreeW(HKEY_LOCAL_MACHINE, key);
    swprintf_s(key, L"Software\\Classes\\CLSID\\%s", clsid); RegDeleteTreeW(HKEY_LOCAL_MACHINE, key);
    swprintf_s(key, L"Software\\Microsoft\\Windows\\CurrentVersion\\Authentication\\Credential Providers\\%s", clsid); RegDeleteTreeW(HKEY_LOCAL_MACHINE, key);

    RemoveWristKeyService();
    return S_OK;
}
