#include "Provider.h"
#include "helpers.h"

#include <credentialprovider.h>
#include <propkey.h>
#include <string>
#include <vector>

#pragma comment(lib, "advapi32.lib")
#pragma comment(lib, "credui.lib")
#pragma comment(lib, "secur32.lib")
#pragma comment(lib, "shlwapi.lib")

static HRESULT CopyString(PCWSTR src, PWSTR* dst) {
    if (!dst) return E_POINTER;
    *dst = nullptr;
    if (!src) return E_INVALIDARG;
    const size_t n = wcslen(src) + 1;
    auto p = static_cast<PWSTR>(CoTaskMemAlloc(n * sizeof(wchar_t)));
    if (!p) return E_OUTOFMEMORY;
    memcpy(p, src, n * sizeof(wchar_t));
    *dst = p;
    return S_OK;
}

// -----------------------------------------------------------------------------
// WristKeyProvider
// -----------------------------------------------------------------------------

WristKeyProvider::WristKeyProvider() = default;

WristKeyProvider::~WristKeyProvider() {
    ReleaseCredentials();
    if (_userArray) {
        _userArray->Release();
        _userArray = nullptr;
    }
    if (_events) {
        _events->Release();
        _events = nullptr;
    }
}

HRESULT STDMETHODCALLTYPE WristKeyProvider::QueryInterface(REFIID riid, void** ppv) {
    if (!ppv) return E_POINTER;
    *ppv = nullptr;

    if (riid == IID_IUnknown || riid == IID_ICredentialProvider) {
        *ppv = static_cast<ICredentialProvider*>(this);
    } else if (riid == IID_ICredentialProviderSetUserArray) {
        *ppv = static_cast<ICredentialProviderSetUserArray*>(this);
    } else {
        return E_NOINTERFACE;
    }

    AddRef();
    return S_OK;
}

ULONG STDMETHODCALLTYPE WristKeyProvider::AddRef() {
    return InterlockedIncrement(&_ref);
}

ULONG STDMETHODCALLTYPE WristKeyProvider::Release() {
    const ULONG n = InterlockedDecrement(&_ref);
    if (n == 0) delete this;
    return n;
}

HRESULT STDMETHODCALLTYPE WristKeyProvider::SetUsageScenario(CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus, DWORD) {
    switch (cpus) {
    case CPUS_LOGON:
    case CPUS_UNLOCK_WORKSTATION:
        // Match Microsoft's V2 sample: these are the two LogonUI scenarios
        // needed for normal sign-in and workstation unlock.
        _scenario = cpus;
        _recreateCredentials = true;
        return S_OK;
    case CPUS_CHANGE_PASSWORD:
    case CPUS_CREDUI:
        return E_NOTIMPL;
    default:
        return E_INVALIDARG;
    }
}

HRESULT STDMETHODCALLTYPE WristKeyProvider::SetSerialization(const CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION*) {
    return E_NOTIMPL;
}

HRESULT STDMETHODCALLTYPE WristKeyProvider::Advise(ICredentialProviderEvents* pcpe, UINT_PTR context) {
    if (_events) _events->Release();
    _events = pcpe;
    _adviseContext = context;
    if (_events) _events->AddRef();
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProvider::UnAdvise() {
    if (_events) {
        _events->Release();
        _events = nullptr;
    }
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProvider::GetFieldDescriptorCount(DWORD* count) {
    if (!count) return E_POINTER;
    *count = WristKeyFields::Count;
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProvider::GetFieldDescriptorAt(DWORD index, CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR** out) {
    if (!out) return E_POINTER;
    *out = nullptr;
    if (index >= WristKeyFields::Count) return E_INVALIDARG;

    CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR d{};
    d.dwFieldID = index;
    d.cpft = CPFT_TILE_IMAGE;
    d.guidFieldType = GUID_NULL;

    switch (index) {
    case WristKeyFields::TileImage:
        d.cpft = CPFT_TILE_IMAGE;
        d.guidFieldType = CPFG_CREDENTIAL_PROVIDER_LOGO;
        return CopyString(L"WristKey", &d.pszLabel) == S_OK
            ? [&]() -> HRESULT {
                auto* copy = static_cast<CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR*>(CoTaskMemAlloc(sizeof(d)));
                if (!copy) { CoTaskMemFree(d.pszLabel); return E_OUTOFMEMORY; }
                *copy = d; *out = copy; return S_OK;
              }() : E_OUTOFMEMORY;
    case WristKeyFields::Tile:
        d.cpft = CPFT_LARGE_TEXT;
        return CopyString(L"WristKey", &d.pszLabel) == S_OK
            ? [&]() -> HRESULT {
                auto* copy = static_cast<CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR*>(CoTaskMemAlloc(sizeof(d)));
                if (!copy) { CoTaskMemFree(d.pszLabel); return E_OUTOFMEMORY; }
                *copy = d; *out = copy; return S_OK;
              }() : E_OUTOFMEMORY;
    case WristKeyFields::Status:
        d.cpft = CPFT_SMALL_TEXT;
        return CopyString(L"Confirm on your watch", &d.pszLabel) == S_OK
            ? [&]() -> HRESULT {
                auto* copy = static_cast<CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR*>(CoTaskMemAlloc(sizeof(d)));
                if (!copy) { CoTaskMemFree(d.pszLabel); return E_OUTOFMEMORY; }
                *copy = d; *out = copy; return S_OK;
              }() : E_OUTOFMEMORY;
    case WristKeyFields::Submit:
        d.cpft = CPFT_SUBMIT_BUTTON;
        return CopyString(L"Unlock", &d.pszLabel) == S_OK
            ? [&]() -> HRESULT {
                auto* copy = static_cast<CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR*>(CoTaskMemAlloc(sizeof(d)));
                if (!copy) { CoTaskMemFree(d.pszLabel); return E_OUTOFMEMORY; }
                *copy = d; *out = copy; return S_OK;
              }() : E_OUTOFMEMORY;
    default:
        return E_INVALIDARG;
    }
}

HRESULT STDMETHODCALLTYPE WristKeyProvider::SetUserArray(ICredentialProviderUserArray* users) {
    if (_userArray) {
        _userArray->Release();
        _userArray = nullptr;
    }

    _userArray = users;
    if (_userArray) _userArray->AddRef();
    _recreateCredentials = true;
    return S_OK;
}

void WristKeyProvider::ReleaseCredentials() {
    for (auto* credential : _credentials) {
        if (credential) credential->Release();
    }
    _credentials.clear();
}

HRESULT WristKeyProvider::CreateCredentials() {
    ReleaseCredentials();

    if (!_userArray) return S_OK;

    DWORD userCount = 0;
    HRESULT hr = _userArray->GetCount(&userCount);
    if (FAILED(hr)) return hr;

    for (DWORD i = 0; i < userCount; ++i) {
        ICredentialProviderUser* user = nullptr;
        hr = _userArray->GetAt(i, &user);
        if (FAILED(hr) || !user) continue;

        PWSTR username = nullptr;
        PWSTR sid = nullptr;

        HRESULT userHr = user->GetStringValue(PKEY_Identity_QualifiedUserName, &username);
        if (SUCCEEDED(userHr)) {
            userHr = user->GetSid(&sid);
        }

        if (SUCCEEDED(userHr) && username && sid) {
            auto* credential = new (std::nothrow) WristKeyProviderCredential(
                this, _scenario, username, sid);
            if (credential) {
                _credentials.push_back(credential);
            }
        }

        CoTaskMemFree(username);
        CoTaskMemFree(sid);
        user->Release();
    }

    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProvider::GetCredentialCount(DWORD* count, DWORD* def, BOOL* autoLogon) {
    if (!count || !def || !autoLogon) return E_POINTER;

    *count = 0;
    *def = CREDENTIAL_PROVIDER_NO_DEFAULT;
    *autoLogon = FALSE;

    if (_recreateCredentials) {
        _recreateCredentials = false;
        HRESULT hr = CreateCredentials();
        if (FAILED(hr)) return hr;
    }

    *count = static_cast<DWORD>(_credentials.size());
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProvider::GetCredentialAt(DWORD index, ICredentialProviderCredential** out) {
    if (!out) return E_POINTER;
    *out = nullptr;
    if (index >= _credentials.size() || !_credentials[index]) return E_INVALIDARG;

    return _credentials[index]->QueryInterface(IID_ICredentialProviderCredential,
                                               reinterpret_cast<void**>(out));
}

void WristKeyProvider::RefreshStatus() {
    if (_events) _events->CredentialsChanged(_adviseContext);
}

// -----------------------------------------------------------------------------
// WristKeyProviderCredential
// -----------------------------------------------------------------------------

WristKeyProviderCredential::WristKeyProviderCredential(
    WristKeyProvider* provider,
    CREDENTIAL_PROVIDER_USAGE_SCENARIO scenario,
    std::wstring username,
    std::wstring sid)
    : _provider(provider),
      _scenario(scenario),
      _username(std::move(username)),
      _sid(std::move(sid)) {
    if (_provider) _provider->AddRef();
}

WristKeyProviderCredential::~WristKeyProviderCredential() {
    if (_events) {
        _events->Release();
        _events = nullptr;
    }
    if (_provider) {
        _provider->Release();
        _provider = nullptr;
    }
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::QueryInterface(REFIID riid, void** ppv) {
    if (!ppv) return E_POINTER;
    *ppv = nullptr;

    if (riid == IID_IUnknown || riid == IID_ICredentialProviderCredential) {
        *ppv = static_cast<ICredentialProviderCredential*>(this);
    } else if (riid == IID_ICredentialProviderCredential2) {
        *ppv = static_cast<ICredentialProviderCredential2*>(this);
    } else {
        return E_NOINTERFACE;
    }

    AddRef();
    return S_OK;
}

ULONG STDMETHODCALLTYPE WristKeyProviderCredential::AddRef() {
    return InterlockedIncrement(&_ref);
}

ULONG STDMETHODCALLTYPE WristKeyProviderCredential::Release() {
    const ULONG n = InterlockedDecrement(&_ref);
    if (n == 0) delete this;
    return n;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::Advise(ICredentialProviderCredentialEvents* e) {
    if (_events) _events->Release();
    _events = e;
    if (_events) _events->AddRef();
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::UnAdvise() {
    if (_events) {
        _events->Release();
        _events = nullptr;
    }
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::SetSelected(BOOL* autoLogon) {
    if (!autoLogon) return E_POINTER;
    *autoLogon = FALSE;
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::SetDeselected() {
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetFieldState(
    DWORD id, CREDENTIAL_PROVIDER_FIELD_STATE* state,
    CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE* interactive) {
    if (!state || !interactive || id >= WristKeyFields::Count) return E_INVALIDARG;

    switch (id) {
    case WristKeyFields::TileImage:
    case WristKeyFields::Tile:
        *state = CPFS_DISPLAY_IN_BOTH;
        *interactive = CPFIS_NONE;
        break;
    case WristKeyFields::Status:
        *state = CPFS_DISPLAY_IN_SELECTED_TILE;
        *interactive = CPFIS_NONE;
        break;
    case WristKeyFields::Submit:
        *state = CPFS_DISPLAY_IN_SELECTED_TILE;
        *interactive = CPFIS_FOCUSED;
        break;
    default:
        return E_INVALIDARG;
    }
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetStringValue(DWORD id, PWSTR* value) {
    if (!value) return E_POINTER;
    *value = nullptr;

    if (id == WristKeyFields::Tile) return CopyString(_username.c_str(), value);
    if (id == WristKeyFields::Status) return CopyString(L"Confirm on your watch", value);
    return E_INVALIDARG;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetBitmapValue(DWORD id, HBITMAP* bitmap) {
    if (!bitmap) return E_POINTER;
    *bitmap = nullptr;
    if (id != WristKeyFields::TileImage) return E_INVALIDARG;

    HICON icon = LoadIconW(nullptr, IDI_INFORMATION);
    if (!icon) return HRESULT_FROM_WIN32(GetLastError());

    ICONINFO info{};
    if (!GetIconInfo(icon, &info)) {
        DestroyIcon(icon);
        return HRESULT_FROM_WIN32(GetLastError());
    }
    DestroyIcon(icon);

    if (info.hbmMask) DeleteObject(info.hbmMask);
    if (!info.hbmColor) return E_FAIL;
    *bitmap = info.hbmColor;
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetCheckboxValue(DWORD, BOOL* checked, LPWSTR* label) {
    if (checked) *checked = FALSE;
    if (label) *label = nullptr;
    return E_NOTIMPL;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetSubmitButtonValue(DWORD id, DWORD* adjacent) {
    if (!adjacent || id != WristKeyFields::Submit) return E_INVALIDARG;
    *adjacent = WristKeyFields::Status;
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetComboBoxValueCount(DWORD, DWORD* count, DWORD* selected) {
    if (count) *count = 0;
    if (selected) *selected = 0;
    return E_NOTIMPL;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetComboBoxValueAt(DWORD, DWORD, PWSTR* item) {
    if (item) *item = nullptr;
    return E_NOTIMPL;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::SetStringValue(DWORD, PCWSTR) {
    return E_NOTIMPL;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::SetCheckboxValue(DWORD, BOOL) {
    return E_NOTIMPL;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::SetComboBoxSelectedValue(DWORD, DWORD) {
    return E_NOTIMPL;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::CommandLinkClicked(DWORD) {
    return E_NOTIMPL;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetUserSid(PWSTR* sid) {
    if (!sid) return E_POINTER;
    *sid = nullptr;
    if (_sid.empty()) return E_FAIL;
    return CopyString(_sid.c_str(), sid);
}

// -----------------------------------------------------------------------------
// BLE unlock / Microsoft-style Windows credential serialization
// -----------------------------------------------------------------------------

static std::wstring Utf8ToWide(const std::string& text) {
    const int wideLength = MultiByteToWideChar(CP_UTF8, 0, text.c_str(),
                                               static_cast<int>(text.size()),
                                               nullptr, 0);
    if (wideLength <= 0) return std::wstring();
    std::wstring wide(static_cast<size_t>(wideLength), L'\0');
    MultiByteToWideChar(CP_UTF8, 0, text.c_str(), static_cast<int>(text.size()),
                        &wide[0], wideLength);
    return wide;
}

// Connects to the WristKey daemon pipe, runs the BLE challenge against the
// watch, and on success receives the DPAPI-decrypted Windows password.
// On failure fills `outError` with a user-readable reason.
static bool RequestUnlockFromDaemon(std::wstring& outPassword, std::wstring& outError) {
    const wchar_t* pipeName = L"\\\\.\\pipe\\wristkey";

    if (!WaitNamedPipeW(pipeName, 2000)) {
        outError = L"WristKey daemon is not running.";
        return false;
    }

    HANDLE pipe = CreateFileW(pipeName, GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                              OPEN_EXISTING, 0, nullptr);
    if (pipe == INVALID_HANDLE_VALUE) {
        outError = L"WristKey: cannot connect to the daemon.";
        return false;
    }

    DWORD mode = PIPE_READMODE_BYTE;
    SetNamedPipeHandleState(pipe, &mode, nullptr, nullptr);

    static const char request[] = "{\"action\":\"unlock\"}\n";
    DWORD written = 0;
    if (!WriteFile(pipe, request, static_cast<DWORD>(strlen(request)), &written, nullptr)) {
        CloseHandle(pipe);
        outError = L"WristKey: daemon communication failed.";
        return false;
    }

    std::string response;
    char buffer[512];
    const DWORD deadline = GetTickCount() + 15000;
    bool timedOut = false;

    for (;;) {
        DWORD available = 0;
        if (PeekNamedPipe(pipe, nullptr, 0, nullptr, &available, nullptr) && available == 0) {
            if (GetTickCount() > deadline) {
                timedOut = true;
                break;
            }
            Sleep(100);
            continue;
        }

        DWORD read = 0;
        if (!ReadFile(pipe, buffer, sizeof(buffer) - 1, &read, nullptr) || read == 0) break;
        buffer[read] = '\0';
        response.append(buffer, read);
        if (response.find('\n') != std::string::npos) break;
        if (GetTickCount() > deadline) { timedOut = true; break; }
    }

    CloseHandle(pipe);

    if (timedOut && response.empty()) {
        outError = L"WristKey: daemon did not answer.";
        return false;
    }

    if (response.find("\"status\":\"success\"") == std::string::npos) {
        // Surface the daemon's own reason (watch timeout, no password
        // configured, no paired device, ...) instead of a generic message.
        static const std::string messageKey = "\"message\":\"";
        size_t start = response.find(messageKey);
        if (start != std::string::npos) {
            start += messageKey.size();
            const size_t end = response.find('"', start);
            if (end != std::string::npos) {
                outError = L"WristKey: " + Utf8ToWide(response.substr(start, end - start)) + L".";
                return false;
            }
        }
        outError = L"WristKey: the watch did not confirm the unlock.";
        return false;
    }

    const std::string key = "\"password\":\"";
    size_t start = response.find(key);
    if (start == std::string::npos) {
        outError = L"WristKey: daemon returned no password.";
        return false;
    }
    start += key.size();
    const size_t end = response.find('"', start);
    if (end == std::string::npos) {
        outError = L"WristKey: daemon returned a malformed response.";
        return false;
    }

    const std::string password = response.substr(start, end - start);
    outPassword = Utf8ToWide(password);
    if (outPassword.empty()) {
        outError = L"WristKey: daemon returned a malformed password.";
        return false;
    }
    return true;
}

static HRESULT PackUnlockCredential(
    const std::wstring& qualifiedUsername,
    const std::wstring& password,
    CREDENTIAL_PROVIDER_USAGE_SCENARIO scenario,
    CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* out) {
    if (!out) return E_POINTER;
    ZeroMemory(out, sizeof(*out));

    if (qualifiedUsername.empty()) return E_INVALIDARG;

    PWSTR domain = nullptr;
    PWSTR username = nullptr;
    PWSTR protectedPassword = nullptr;
    HRESULT hr = SplitDomainAndUsername(qualifiedUsername.c_str(), &domain, &username);

    if (SUCCEEDED(hr)) {
        hr = ProtectIfNecessaryAndCopyPassword(password.c_str(), scenario, &protectedPassword);
    }

    if (SUCCEEDED(hr)) {
        KERB_INTERACTIVE_UNLOCK_LOGON kiul{};
        hr = KerbInteractiveUnlockLogonInit(
            domain,
            username,
            protectedPassword,
            scenario,
            &kiul);

        if (SUCCEEDED(hr)) {
            hr = KerbInteractiveUnlockLogonPack(
                kiul,
                &out->rgbSerialization,
                &out->cbSerialization);
        }

        if (SUCCEEDED(hr)) {
            ULONG authPackage = 0;
            hr = RetrieveNegotiateAuthPackage(&authPackage);
            if (SUCCEEDED(hr)) {
                out->ulAuthenticationPackage = authPackage;
                out->clsidCredentialProvider = CLSID_WristKeyCredentialProvider;
            }
        }
    }

    CoTaskMemFree(domain);
    CoTaskMemFree(username);

    if (protectedPassword) {
        SecureZeroMemory(protectedPassword, wcslen(protectedPassword) * sizeof(wchar_t));
        CoTaskMemFree(protectedPassword);
    }

    if (FAILED(hr)) {
        CoTaskMemFree(out->rgbSerialization);
        ZeroMemory(out, sizeof(*out));
    }

    return hr;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetSerialization(
    CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE* response,
    CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* serialization,
    PWSTR* status,
    CREDENTIAL_PROVIDER_STATUS_ICON* icon) {
    if (!response || !serialization || !status || !icon) return E_POINTER;

    ZeroMemory(serialization, sizeof(*serialization));
    *response = CPGSR_NO_CREDENTIAL_NOT_FINISHED;
    *status = nullptr;
    *icon = CPSI_NONE;

    std::wstring password;
    std::wstring daemonError;
    if (!RequestUnlockFromDaemon(password, daemonError)) {
        CopyString((daemonError.empty()
                        ? L"WristKey: confirm the unlock on your watch."
                        : daemonError.c_str()),
                   status);
        *icon = CPSI_WARNING;
        return S_OK;
    }

    HRESULT hr = PackUnlockCredential(_username, password, _scenario, serialization);

    if (!password.empty()) {
        SecureZeroMemory(password.data(), password.size() * sizeof(wchar_t));
    }

    if (SUCCEEDED(hr)) {
        *response = CPGSR_RETURN_CREDENTIAL_FINISHED;
        *icon = CPSI_SUCCESS;
    } else {
        CopyString(L"WristKey: Windows could not serialize the credential.", status);
        *icon = CPSI_ERROR;
    }

    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::ReportResult(
    NTSTATUS ntsStatus,
    NTSTATUS ntsSubstatus,
    PWSTR* status,
    CREDENTIAL_PROVIDER_STATUS_ICON* icon) {
    if (status) *status = nullptr;
    if (icon) *icon = CPSI_NONE;

    if (ntsStatus != 0) {
        if (status) CopyString(L"WristKey: Windows rejected the credential.", status);
        if (icon) *icon = CPSI_ERROR;
    }
    return S_OK;
}
