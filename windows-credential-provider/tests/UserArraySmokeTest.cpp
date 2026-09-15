#include <windows.h>
#include <credentialprovider.h>
#include <propkey.h>
#include <propvarutil.h>
#include <iostream>
#include <string>
#include <new>

using DllGetClassObjectFn = HRESULT (STDAPICALLTYPE*)(REFCLSID, REFIID, void**);

static const GUID CLSID_WristKeyCredentialProvider =
    {0x7E1B7B8A, 0x4C8B, 0x4C2F, {0x9D, 0x8A, 0x8D, 0x3A, 0x7F, 0x2E, 0x51, 0xA1}};

static const GUID Identity_LocalUserProvider =
    {0xA198529B, 0x730F, 0x4089, {0xB6, 0x46, 0xA1, 0x25, 0x57, 0xF5, 0x66, 0x5E}};

static HRESULT CopyString(const std::wstring& value, PWSTR* out) {
    if (!out) return E_POINTER;
    *out = nullptr;
    const size_t bytes = (value.size() + 1) * sizeof(wchar_t);
    auto* buffer = static_cast<PWSTR>(CoTaskMemAlloc(bytes));
    if (!buffer) return E_OUTOFMEMORY;
    memcpy(buffer, value.c_str(), bytes);
    *out = buffer;
    return S_OK;
}

class FakeUser final : public ICredentialProviderUser {
public:
    FakeUser(std::wstring username, std::wstring qualifiedUsername, std::wstring sid)
        : username_(std::move(username)),
          qualifiedUsername_(std::move(qualifiedUsername)),
          sid_(std::move(sid)) {}

    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void** ppv) override {
        if (!ppv) return E_POINTER;
        *ppv = nullptr;
        if (riid == IID_IUnknown || riid == __uuidof(ICredentialProviderUser)) {
            *ppv = static_cast<ICredentialProviderUser*>(this);
            AddRef();
            return S_OK;
        }
        return E_NOINTERFACE;
    }

    ULONG STDMETHODCALLTYPE AddRef() override { return InterlockedIncrement(&ref_); }
    ULONG STDMETHODCALLTYPE Release() override {
        ULONG count = InterlockedDecrement(&ref_);
        if (!count) delete this;
        return count;
    }

    HRESULT STDMETHODCALLTYPE GetSid(PWSTR* sid) override {
        return CopyString(sid_, sid);
    }

    HRESULT STDMETHODCALLTYPE GetProviderID(GUID* providerID) override {
        if (!providerID) return E_POINTER;
        *providerID = Identity_LocalUserProvider;
        return S_OK;
    }

    HRESULT STDMETHODCALLTYPE GetStringValue(REFPROPERTYKEY key, PWSTR* value) override {
        if (key == PKEY_Identity_UserName) return CopyString(username_, value);
        if (key == PKEY_Identity_QualifiedUserName) return CopyString(qualifiedUsername_, value);
        if (key == PKEY_Identity_DisplayName) return CopyString(username_, value);
        if (key == PKEY_Identity_PrimarySid) return CopyString(sid_, value);
        return E_INVALIDARG;
    }

    HRESULT STDMETHODCALLTYPE GetValue(REFPROPERTYKEY key, PROPVARIANT* value) override {
        if (!value) return E_POINTER;
        PropVariantInit(value);
        if (key == PKEY_Identity_UserName ||
            key == PKEY_Identity_QualifiedUserName ||
            key == PKEY_Identity_DisplayName ||
            key == PKEY_Identity_PrimarySid) {
            PWSTR stringValue = nullptr;
            HRESULT hr = GetStringValue(key, &stringValue);
            if (FAILED(hr)) return hr;
            hr = InitPropVariantFromString(stringValue, value);
            CoTaskMemFree(stringValue);
            return hr;
        }
        return E_INVALIDARG;
    }

private:
    LONG ref_ = 1;
    std::wstring username_;
    std::wstring qualifiedUsername_;
    std::wstring sid_;
};

class FakeUserArray final : public ICredentialProviderUserArray {
public:
    explicit FakeUserArray(ICredentialProviderUser* user) : user_(user) {
        if (user_) user_->AddRef();
    }

    ~FakeUserArray() {
        if (user_) user_->Release();
    }

    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void** ppv) override {
        if (!ppv) return E_POINTER;
        *ppv = nullptr;
        if (riid == IID_IUnknown || riid == __uuidof(ICredentialProviderUserArray)) {
            *ppv = static_cast<ICredentialProviderUserArray*>(this);
            AddRef();
            return S_OK;
        }
        return E_NOINTERFACE;
    }

    ULONG STDMETHODCALLTYPE AddRef() override { return InterlockedIncrement(&ref_); }
    ULONG STDMETHODCALLTYPE Release() override {
        ULONG count = InterlockedDecrement(&ref_);
        if (!count) delete this;
        return count;
    }

    HRESULT STDMETHODCALLTYPE SetProviderFilter(REFGUID guidProviderToFilterTo) override {
        return guidProviderToFilterTo == Identity_LocalUserProvider ? S_OK : E_INVALIDARG;
    }

    HRESULT STDMETHODCALLTYPE GetAccountOptions(CREDENTIAL_PROVIDER_ACCOUNT_OPTIONS* options) override {
        if (!options) return E_POINTER;
        *options = CREDENTIAL_PROVIDER_ACCOUNT_OPTIONS_NONE;
        return S_OK;
    }

    HRESULT STDMETHODCALLTYPE GetCount(DWORD* count) override {
        if (!count) return E_POINTER;
        *count = user_ ? 1 : 0;
        return S_OK;
    }

    HRESULT STDMETHODCALLTYPE GetAt(DWORD index, ICredentialProviderUser** user) override {
        if (!user) return E_POINTER;
        *user = nullptr;
        if (index != 0 || !user_) return E_INVALIDARG;
        user_->AddRef();
        *user = user_;
        return S_OK;
    }

private:
    LONG ref_ = 1;
    ICredentialProviderUser* user_ = nullptr;
};

static int Fail(const wchar_t* label, HRESULT hr, int code) {
    std::wcerr << label << L" 0x" << std::hex << static_cast<unsigned long>(hr) << L"\n";
    return code;
}

int wmain(int argc, wchar_t** argv) {
    if (argc != 2) {
        std::wcerr << L"Usage: UserArraySmokeTest.exe <path-to-WristKeyCredentialProvider.dll>\n";
        return 2;
    }

    HRESULT hr = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (FAILED(hr)) return Fail(L"CoInitializeEx failed:", hr, 3);

    HMODULE module = LoadLibraryW(argv[1]);
    if (!module) {
        hr = HRESULT_FROM_WIN32(GetLastError());
        CoUninitialize();
        return Fail(L"LoadLibraryW failed:", hr, 4);
    }

    auto cleanup = [&]() {
        FreeLibrary(module);
        CoUninitialize();
    };

    auto getClassObject = reinterpret_cast<DllGetClassObjectFn>(
        GetProcAddress(module, "DllGetClassObject"));
    if (!getClassObject) {
        std::wcerr << L"DllGetClassObject export not found.\n";
        cleanup();
        return 5;
    }

    IClassFactory* factory = nullptr;
    hr = getClassObject(CLSID_WristKeyCredentialProvider, IID_IClassFactory,
                        reinterpret_cast<void**>(&factory));
    if (FAILED(hr) || !factory) {
        int code = Fail(L"DllGetClassObject failed:", hr, 6);
        cleanup();
        return code;
    }

    ICredentialProvider* provider = nullptr;
    hr = factory->CreateInstance(nullptr, IID_ICredentialProvider,
                                 reinterpret_cast<void**>(&provider));
    factory->Release();
    if (FAILED(hr) || !provider) {
        int code = Fail(L"CreateInstance failed:", hr, 7);
        cleanup();
        return code;
    }

    hr = provider->SetUsageScenario(CPUS_UNLOCK_WORKSTATION, 0);
    if (FAILED(hr)) {
        int code = Fail(L"SetUsageScenario failed:", hr, 8);
        provider->Release();
        cleanup();
        return code;
    }

    auto* fakeUser = new (std::nothrow) FakeUser(
        L"WristKeyTestUser",
        L"WristKeyTestUser",
        L"S-1-5-21-111111111-222222222-333333333-1001");
    if (!fakeUser) {
        provider->Release();
        cleanup();
        return 9;
    }

    auto* fakeArray = new (std::nothrow) FakeUserArray(fakeUser);
    if (!fakeArray) {
        fakeUser->Release();
        provider->Release();
        cleanup();
        return 10;
    }

    ICredentialProviderSetUserArray* setUsers = nullptr;
    hr = provider->QueryInterface(IID_ICredentialProviderSetUserArray,
                                  reinterpret_cast<void**>(&setUsers));
    if (FAILED(hr) || !setUsers) {
        int code = Fail(L"QI SetUserArray failed:", hr, 11);
        fakeArray->Release();
        fakeUser->Release();
        provider->Release();
        cleanup();
        return code;
    }

    hr = setUsers->SetUserArray(fakeArray);
    setUsers->Release();
    fakeArray->Release();
    fakeUser->Release();
    if (FAILED(hr)) {
        int code = Fail(L"SetUserArray failed:", hr, 12);
        provider->Release();
        cleanup();
        return code;
    }

    DWORD count = 0;
    DWORD defaultIndex = CREDENTIAL_PROVIDER_NO_DEFAULT;
    BOOL autoLogon = FALSE;
    hr = provider->GetCredentialCount(&count, &defaultIndex, &autoLogon);
    if (FAILED(hr)) {
        int code = Fail(L"GetCredentialCount failed:", hr, 13);
        provider->Release();
        cleanup();
        return code;
    }
    if (count != 1) {
        std::wcerr << L"Unexpected credential count: " << count << L" (expected 1)\n";
        provider->Release();
        cleanup();
        return 14;
    }

    ICredentialProviderCredential* credential = nullptr;
    hr = provider->GetCredentialAt(0, &credential);
    if (FAILED(hr) || !credential) {
        int code = Fail(L"GetCredentialAt failed:", hr, 15);
        provider->Release();
        cleanup();
        return code;
    }

    ICredentialProviderCredential2* credential2 = nullptr;
    hr = credential->QueryInterface(IID_ICredentialProviderCredential2,
                                    reinterpret_cast<void**>(&credential2));
    if (FAILED(hr) || !credential2) {
        int code = Fail(L"QI ICredentialProviderCredential2 failed:", hr, 16);
        credential->Release();
        provider->Release();
        cleanup();
        return code;
    }

    PWSTR sid = nullptr;
    hr = credential2->GetUserSid(&sid);
    if (FAILED(hr) || !sid) {
        int code = Fail(L"GetUserSid failed:", hr, 17);
        credential2->Release();
        credential->Release();
        provider->Release();
        cleanup();
        return code;
    }

    const std::wstring expectedSid = L"S-1-5-21-111111111-222222222-333333333-1001";
    if (expectedSid != sid) {
        std::wcerr << L"Unexpected SID: " << sid << L"\n";
        CoTaskMemFree(sid);
        credential2->Release();
        credential->Release();
        provider->Release();
        cleanup();
        return 18;
    }

    PWSTR name = nullptr;
    hr = credential->GetStringValue(1, &name);
    if (FAILED(hr) || !name) {
        int code = Fail(L"GetStringValue(Tile) failed:", hr, 19);
        CoTaskMemFree(sid);
        credential2->Release();
        credential->Release();
        provider->Release();
        cleanup();
        return code;
    }

    std::wcout << L"User array credential smoke test passed.\n";
    std::wcout << L"Credential count: " << count << L"\n";
    std::wcout << L"Credential username: " << name << L"\n";
    std::wcout << L"Credential SID: " << sid << L"\n";

    CoTaskMemFree(name);
    CoTaskMemFree(sid);
    credential2->Release();
    credential->Release();
    provider->Release();
    cleanup();
    return 0;
}
