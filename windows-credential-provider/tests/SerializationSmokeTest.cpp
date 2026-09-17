#include <windows.h>
#include <credentialprovider.h>
#include <ntsecapi.h>
#include <wincred.h>
#include <initguid.h>  // must precede propkey.h so PKEY_Identity_* get defined
#include <propkey.h>
#include <propvarutil.h>
#include <iostream>
#include <string>
#include <thread>
#include <atomic>

using DllGetClassObjectFn = HRESULT (STDAPICALLTYPE*)(REFCLSID, REFIID, void**);
using DllCanUnloadNowFn = HRESULT (STDAPICALLTYPE*)();

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
        : username_(std::move(username)), qualifiedUsername_(std::move(qualifiedUsername)), sid_(std::move(sid)) {}

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

    HRESULT STDMETHODCALLTYPE GetSid(PWSTR* sid) override { return CopyString(sid_, sid); }

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

        if (key == PKEY_Identity_UserName || key == PKEY_Identity_QualifiedUserName ||
            key == PKEY_Identity_DisplayName || key == PKEY_Identity_PrimarySid) {
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

    HRESULT STDMETHODCALLTYPE SetProviderFilter(REFGUID) override { return S_OK; }

    HRESULT STDMETHODCALLTYPE GetAccountOptions(CREDENTIAL_PROVIDER_ACCOUNT_OPTIONS* options) override {
        if (!options) return E_POINTER;
        *options = static_cast<CREDENTIAL_PROVIDER_ACCOUNT_OPTIONS>(0);
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

struct PipeServerResult {
    std::atomic<bool> ready{false};
    std::atomic<bool> connected{false};
    std::atomic<bool> validRequest{false};
    DWORD error = ERROR_SUCCESS;
};

static void RunTestPipeServer(PipeServerResult* result) {
    constexpr wchar_t kPipeName[] = L"\\\\.\\pipe\\wristkey";
    // JSON with escaped characters (\\ -> '\', \" -> '"', \u0442... -> Cyrillic
    // "тест"). A naive parser would cut the password at the first '"'.
    constexpr char kResponse[] =
        "{\"status\":\"success\",\"password\":\"TestPassword123! \\\\\"quoted\\\\\" "
        "\\u0442\\u0435\\u0441\\u0442\"}\n";

    HANDLE pipe = CreateNamedPipeW(
        kPipeName,
        PIPE_ACCESS_DUPLEX,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
        1,
        1024,
        1024,
        5000,
        nullptr);

    if (pipe == INVALID_HANDLE_VALUE) {
        result->error = GetLastError();
        result->ready = true;
        return;
    }

    result->ready = true;

    BOOL connected = ConnectNamedPipe(pipe, nullptr)
        ? TRUE
        : (GetLastError() == ERROR_PIPE_CONNECTED ? TRUE : FALSE);

    if (!connected) {
        result->error = GetLastError();
        CloseHandle(pipe);
        return;
    }

    result->connected = true;

    char request[256]{};
    DWORD read = 0;
    if (!ReadFile(pipe, request, sizeof(request) - 1, &read, nullptr) || read == 0) {
        result->error = GetLastError();
        FlushFileBuffers(pipe);
        DisconnectNamedPipe(pipe);
        CloseHandle(pipe);
        return;
    }

    request[read] = '\0';
    std::string text(request, read);
    result->validRequest = text.find("{\"action\":\"unlock\"}") != std::string::npos;

    DWORD written = 0;
    if (!WriteFile(pipe, kResponse, static_cast<DWORD>(strlen(kResponse)), &written, nullptr)) {
        result->error = GetLastError();
    }

    FlushFileBuffers(pipe);
    DisconnectNamedPipe(pipe);
    CloseHandle(pipe);
}

static std::wstring UnpackPackedString(
    const KERB_INTERACTIVE_UNLOCK_LOGON* packed,
    const UNICODE_STRING& value) {
    if (!packed || !value.Buffer || value.Length == 0) return {};

    const auto base = reinterpret_cast<const BYTE*>(packed);
    const auto offset = reinterpret_cast<ULONG_PTR>(value.Buffer);
    const auto absolute = base + offset;

    return std::wstring(
        reinterpret_cast<const wchar_t*>(absolute),
        value.Length / sizeof(wchar_t));
}

int wmain(int argc, wchar_t** argv) {
    if (argc != 2) {
        std::wcerr << L"Usage: SerializationSmokeTest.exe <path-to-WristKeyCredentialProvider.dll>\n";
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
    hr = getClassObject(
        CLSID_WristKeyCredentialProvider,
        IID_IClassFactory,
        reinterpret_cast<void**>(&factory));
    if (FAILED(hr) || !factory) {
        int code = Fail(L"DllGetClassObject failed:", hr, 6);
        cleanup();
        return code;
    }

    ICredentialProvider* provider = nullptr;
    hr = factory->CreateInstance(
        nullptr,
        IID_ICredentialProvider,
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
    hr = provider->QueryInterface(
        IID_ICredentialProviderSetUserArray,
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
    if (FAILED(hr) || count != 1) {
        int code = FAILED(hr)
            ? Fail(L"GetCredentialCount failed:", hr, 13)
            : 14;
        if (SUCCEEDED(hr)) std::wcerr << L"Unexpected credential count: " << count << L"\n";
        provider->Release();
        cleanup();
        return code;
    }

    ICredentialProviderCredential* credential = nullptr;
    hr = provider->GetCredentialAt(0, &credential);
    if (FAILED(hr) || !credential) {
        int code = Fail(L"GetCredentialAt failed:", hr, 15);
        provider->Release();
        cleanup();
        return code;
    }

    PipeServerResult pipeResult;
    std::thread pipeThread(RunTestPipeServer, &pipeResult);

    while (!pipeResult.ready.load()) Sleep(10);

    if (pipeResult.error != ERROR_SUCCESS) {
        std::wcerr << L"Test named pipe setup failed: " << pipeResult.error << L"\n";
        pipeThread.join();
        credential->Release();
        provider->Release();
        cleanup();
        return 16;
    }

    CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE response = CPGSR_NO_CREDENTIAL_NOT_FINISHED;
    CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION serialization{};
    PWSTR serializationStatus = nullptr;
    CREDENTIAL_PROVIDER_STATUS_ICON serializationIcon = CPSI_NONE;

    hr = credential->GetSerialization(&response, &serialization, &serializationStatus, &serializationIcon);
    if (serializationStatus) CoTaskMemFree(serializationStatus);

    pipeThread.join();

    if (FAILED(hr)) {
        int code = Fail(L"GetSerialization failed:", hr, 17);
        credential->Release();
        provider->Release();
        cleanup();
        return code;
    }

    if (response != CPGSR_RETURN_CREDENTIAL_FINISHED) {
        std::wcerr << L"Unexpected serialization response: " << static_cast<int>(response) << L"\n";
        credential->Release();
        provider->Release();
        cleanup();
        return 18;
    }

    if (serializationIcon != CPSI_SUCCESS) {
        std::wcerr << L"Unexpected serialization status icon.\n";
        CoTaskMemFree(serialization.rgbSerialization);
        credential->Release();
        provider->Release();
        cleanup();
        return 19;
    }

    if (!serialization.rgbSerialization || serialization.cbSerialization < sizeof(KERB_INTERACTIVE_UNLOCK_LOGON)) {
        std::wcerr << L"Serialization buffer is missing or too small.\n";
        CoTaskMemFree(serialization.rgbSerialization);
        credential->Release();
        provider->Release();
        cleanup();
        return 20;
    }

    if (serialization.ulAuthenticationPackage == 0) {
        std::wcerr << L"Authentication package was not populated.\n";
        CoTaskMemFree(serialization.rgbSerialization);
        credential->Release();
        provider->Release();
        cleanup();
        return 21;
    }

    if (serialization.clsidCredentialProvider != CLSID_WristKeyCredentialProvider) {
        std::wcerr << L"Unexpected credential provider CLSID in serialization.\n";
        CoTaskMemFree(serialization.rgbSerialization);
        credential->Release();
        provider->Release();
        cleanup();
        return 22;
    }

    auto* packed = reinterpret_cast<const KERB_INTERACTIVE_UNLOCK_LOGON*>(
        serialization.rgbSerialization);

    const std::wstring domain = UnpackPackedString(packed, packed->Logon.LogonDomainName);
    const std::wstring username = UnpackPackedString(packed, packed->Logon.UserName);
    const std::wstring protectedPassword = UnpackPackedString(packed, packed->Logon.Password);

    if (packed->Logon.MessageType != KerbWorkstationUnlockLogon) {
        std::wcerr << L"Unexpected Kerberos message type: "
                   << static_cast<int>(packed->Logon.MessageType) << L"\n";
        CoTaskMemFree(serialization.rgbSerialization);
        credential->Release();
        provider->Release();
        cleanup();
        return 23;
    }

    if (domain != L"." || username != L"WristKeyTestUser") {
        std::wcerr << L"Unexpected serialized identity: domain='" << domain
                   << L"' username='" << username << L"'\n";
        CoTaskMemFree(serialization.rgbSerialization);
        credential->Release();
        provider->Release();
        cleanup();
        return 24;
    }

    if (protectedPassword.empty()) {
        std::wcerr << L"Serialized password is empty.\n";
        CoTaskMemFree(serialization.rgbSerialization);
        credential->Release();
        provider->Release();
        cleanup();
        return 25;
    }

    CRED_PROTECTION_TYPE protectionType = CredUnprotected;
    // CredIsProtectedW takes a mutable buffer despite not modifying it.
    std::wstring protectedPasswordBuf = protectedPassword;
    if (!CredIsProtectedW(&protectedPasswordBuf[0], &protectionType) ||
        protectionType == CredUnprotected) {
        std::wcerr << L"Serialized password is not CredProtect-protected.\n";
        CoTaskMemFree(serialization.rgbSerialization);
        credential->Release();
        provider->Release();
        cleanup();
        return 26;
    }

    // Round-trip the protected blob: a naive JSON reader would have decoded a
    // truncated password (e.g. `TestPassword123! \`), and the round-trip below
    // would then differ from the escaped value in the daemon response.
    std::wstring unprotected;
    unprotected.resize(protectedPassword.size() + 64);
    ULONG unprotectedChars = static_cast<ULONG>(unprotected.size());
    if (!CredUnprotectW(&protectedPasswordBuf[0], &unprotected[0], &unprotectedChars)) {
        std::wcerr << L"CredUnprotectW failed: " << GetLastError() << L"\n";
        CoTaskMemFree(serialization.rgbSerialization);
        credential->Release();
        provider->Release();
        cleanup();
        return 28;
    }
    unprotected.resize(unprotectedChars);

    const std::wstring expectedPassword =
        L"TestPassword123! \\\"quoted\\\" \x0442\x0435\x0441\x0442";
    if (unprotected != expectedPassword) {
        std::wcerr << L"Decoded password does not match the escaped daemon response.\n";
        CoTaskMemFree(serialization.rgbSerialization);
        credential->Release();
        provider->Release();
        cleanup();
        return 29;
    }

    if (!pipeResult.connected || !pipeResult.validRequest) {
        std::wcerr << L"Credential did not issue the expected daemon unlock request.\n";
        CoTaskMemFree(serialization.rgbSerialization);
        credential->Release();
        provider->Release();
        cleanup();
        return 27;
    }

    std::wcout << L"Serialization smoke test passed.\n";
    std::wcout << L"Response: CPGSR_RETURN_CREDENTIAL_FINISHED\n";
    std::wcout << L"MessageType: KerbWorkstationUnlockLogon\n";
    std::wcout << L"Domain: " << domain << L"\n";
    std::wcout << L"Username: " << username << L"\n";
    std::wcout << L"Auth package: " << serialization.ulAuthenticationPackage << L"\n";
    std::wcout << L"Password protection: CredProtect\n";
    std::wcout << L"Daemon request: {\"action\":\"unlock\"}\n";

    SecureZeroMemory(serialization.rgbSerialization, serialization.cbSerialization);
    CoTaskMemFree(serialization.rgbSerialization);
    credential->Release();
    provider->Release();
    cleanup();
    return 0;
}
