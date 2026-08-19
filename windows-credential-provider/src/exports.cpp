#include "Provider.h"
#include <windows.h>
#include <shlwapi.h>
#include <new>

extern const GUID CLSID_WristKeyCredentialProvider;

static HMODULE g_module = nullptr;
static LONG g_objects = 0;
static LONG g_locks = 0;

class ClassFactory final : public IClassFactory {
public:
    explicit ClassFactory() : ref_(1) { InterlockedIncrement(&g_objects); }
    ~ClassFactory() override { InterlockedDecrement(&g_objects); }
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

extern "C" HRESULT __declspec(dllexport) DllGetClassObject(REFCLSID clsid, REFIID iid, void** ppv) {
    if (clsid != CLSID_WristKeyCredentialProvider) return CLASS_E_CLASSNOTAVAILABLE;
    auto* f = new (std::nothrow) ClassFactory();
    if (!f) return E_OUTOFMEMORY;
    HRESULT hr = f->QueryInterface(iid, ppv);
    f->Release();
    return hr;
}

extern "C" HRESULT __declspec(dllexport) DllCanUnloadNow() {
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

extern "C" HRESULT __declspec(dllexport) DllRegisterServer() {
    wchar_t path[MAX_PATH]{};
    if (!GetModuleFileNameW(g_module, path, ARRAYSIZE(path))) return HRESULT_FROM_WIN32(GetLastError());
    wchar_t clsid[64]{};
    StringFromGUID2(CLSID_WristKeyCredentialProvider, clsid, ARRAYSIZE(clsid));

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
    return WriteClsidKey(HKEY_LOCAL_MACHINE, key, L"WristKey Credential Provider");
}

extern "C" HRESULT __declspec(dllexport) DllUnregisterServer() {
    wchar_t clsid[64]{};
    StringFromGUID2(CLSID_WristKeyCredentialProvider, clsid, ARRAYSIZE(clsid));
    wchar_t key[MAX_PATH]{};
    swprintf_s(key, L"Software\\Classes\\CLSID\\%s\\InprocServer32", clsid); RegDeleteTreeW(HKEY_LOCAL_MACHINE, key);
    swprintf_s(key, L"Software\\Classes\\CLSID\\%s", clsid); RegDeleteTreeW(HKEY_LOCAL_MACHINE, key);
    swprintf_s(key, L"Software\\Microsoft\\Windows\\CurrentVersion\\Authentication\\Credential Providers\\%s", clsid); RegDeleteTreeW(HKEY_LOCAL_MACHINE, key);
    return S_OK;
}
