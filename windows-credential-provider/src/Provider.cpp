#include "Provider.h"
#include <new>
#include <shlwapi.h>
#include <lm.h>
#include <string>
#include <wincred.h>
#include <ntsecapi.h>

#pragma comment(lib, "credui.lib")
#pragma comment(lib, "secur32.lib")

extern const GUID CLSID_WristKeyCredentialProvider;

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

static std::vector<std::wstring> EnumerateLocalUsers() {
    std::vector<std::wstring> users;
    DWORD level = 1;
    LPUSER_INFO_1 info = nullptr;
    DWORD entries = 0, total = 0, resume = 0;
    NET_API_STATUS status;
    do {
        status = NetUserEnum(nullptr, level, FILTER_NORMAL_ACCOUNT, reinterpret_cast<LPBYTE*>(&info),
                             MAX_PREFERRED_LENGTH, &entries, &total, &resume);
        if (status != NERR_Success && status != ERROR_MORE_DATA) break;
        for (DWORD i = 0; i < entries; ++i) {
            if (!info[i].usri1_name) continue;
            if (info[i].usri1_flags & UF_ACCOUNTDISABLE) continue;
            users.emplace_back(info[i].usri1_name);
        }
        if (info) {
            NetApiBufferFree(info);
            info = nullptr;
        }
    } while (status == ERROR_MORE_DATA);
    if (info) NetApiBufferFree(info);
    if (users.empty()) users.emplace_back(L"Current Windows user");
    return users;
}

WristKeyProvider::WristKeyProvider() {
    const auto users = EnumerateLocalUsers();
    for (const auto& user : users) {
        auto* credential = new (std::nothrow) WristKeyProviderCredential(this, user);
        if (credential) _credentials.push_back(credential);
    }
}

WristKeyProvider::~WristKeyProvider() {
    if (_events) _events->Release();
    for (auto* credential : _credentials) {
        if (credential) credential->Release();
    }
}

HRESULT STDMETHODCALLTYPE WristKeyProvider::QueryInterface(REFIID riid, void** ppv) {
    if (!ppv) return E_POINTER;
    *ppv = nullptr;
    if (riid == IID_IUnknown || riid == IID_ICredentialProvider) {
        *ppv = static_cast<ICredentialProvider*>(this);
        AddRef();
        return S_OK;
    }
    return E_NOINTERFACE;
}
ULONG STDMETHODCALLTYPE WristKeyProvider::AddRef() { return InterlockedIncrement(&_ref); }
ULONG STDMETHODCALLTYPE WristKeyProvider::Release() {
    const auto n = InterlockedDecrement(&_ref);
    if (!n) delete this;
    return n;
}

HRESULT STDMETHODCALLTYPE WristKeyProvider::SetUsageScenario(CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus, DWORD) {
    if (cpus != CPUS_LOGON && cpus != CPUS_UNLOCK_WORKSTATION) return E_NOTIMPL;
    _scenario = cpus;
    return S_OK;
}
HRESULT STDMETHODCALLTYPE WristKeyProvider::SetSerialization(const CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION*) { return E_NOTIMPL; }
HRESULT STDMETHODCALLTYPE WristKeyProvider::Advise(ICredentialProviderEvents* pcpe, UINT_PTR context) {
    if (_events) _events->Release();
    _events = pcpe;
    _adviseContext = context;
    if (_events) _events->AddRef();
    return S_OK;
}
HRESULT STDMETHODCALLTYPE WristKeyProvider::UnAdvise() {
    if (_events) { _events->Release(); _events = nullptr; }
    return S_OK;
}
HRESULT STDMETHODCALLTYPE WristKeyProvider::GetFieldDescriptorCount(DWORD* pdwCount) {
    if (!pdwCount) return E_POINTER;
    *pdwCount = WristKeyFields::Count;
    return S_OK;
}
HRESULT STDMETHODCALLTYPE WristKeyProvider::GetFieldDescriptorAt(DWORD index, ICredentialProviderFieldDescriptor** out) {
    if (!out) return E_POINTER;
    *out = nullptr;
    if (index >= WristKeyFields::Count) return E_INVALIDARG;
    CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR d{};
    d.dwFieldID = index;
    d.cpft = (index == WristKeyFields::Tile) ? CPFT_LARGE_TEXT :
             (index == WristKeyFields::Status) ? CPFT_SMALL_TEXT : CPFT_SUBMIT_BUTTON;
    d.pszLabel = const_cast<PWSTR>(index == WristKeyFields::Tile ? L"WristKey" :
                                    index == WristKeyFields::Status ? L"Select account and unlock with watch" : L"Unlock");
    return SHCreateCredentialProviderFieldDescriptor(&d, out);
}
HRESULT STDMETHODCALLTYPE WristKeyProvider::GetCredentialCount(DWORD* count, DWORD* def, BOOL* autoLogon) {
    if (!count || !def || !autoLogon) return E_POINTER;
    *count = static_cast<DWORD>(_credentials.size());
    *def = CREDENTIAL_PROVIDER_NO_DEFAULT;
    *autoLogon = FALSE;
    return S_OK;
}
HRESULT STDMETHODCALLTYPE WristKeyProvider::GetCredentialAt(DWORD index, ICredentialProviderCredential** out) {
    if (!out) return E_POINTER;
    *out = nullptr;
    if (index >= _credentials.size() || !_credentials[index]) return E_INVALIDARG;
    _credentials[index]->AddRef();
    *out = _credentials[index];
    return S_OK;
}
void WristKeyProvider::SetEvents(ICredentialProviderCredentialEvents*, UINT_PTR) {}
bool WristKeyProvider::WatchAvailable() const { return false; }
void WristKeyProvider::RefreshStatus() {}

WristKeyProviderCredential::WristKeyProviderCredential(WristKeyProvider* provider, std::wstring username)
    : _provider(provider), _username(std::move(username)) {
    if (_provider) _provider->AddRef();
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::QueryInterface(REFIID riid, void** ppv) {
    if (!ppv) return E_POINTER;
    *ppv = nullptr;
    if (riid == IID_IUnknown || riid == IID_ICredentialProviderCredential) {
        *ppv = static_cast<ICredentialProviderCredential*>(this);
        AddRef();
        return S_OK;
    }
    return E_NOINTERFACE;
}
ULONG STDMETHODCALLTYPE WristKeyProviderCredential::AddRef() { return InterlockedIncrement(&_ref); }
ULONG STDMETHODCALLTYPE WristKeyProviderCredential::Release() {
    const auto n = InterlockedDecrement(&_ref);
    if (!n) delete this;
    return n;
}
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::Advise(ICredentialProviderCredentialEvents* e) {
    if (_events) _events->Release();
    _events = e;
    if (_events) _events->AddRef();
    return S_OK;
}
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::UnAdvise() { if (_events) { _events->Release(); _events = nullptr; } return S_OK; }
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::SetSelected(BOOL* autoLogon) { if (autoLogon) *autoLogon = FALSE; return S_OK; }
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::SetDeselected() { return S_OK; }
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetFieldState(DWORD id, CREDENTIAL_PROVIDER_FIELD_STATE* state, CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE* interactive) {
    if (!state || !interactive || id >= WristKeyFields::Count) return E_INVALIDARG;
    *state = CPFS_DISPLAY_IN_SELECTED_TILE;
    *interactive = (id == WristKeyFields::Submit) ? CPFIS_FOCUSED : CPFIS_NONE;
    return S_OK;
}
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetStringValue(DWORD id, PWSTR* value) {
    if (id == WristKeyFields::Tile) return CopyString(_username.c_str(), value);
    if (id == WristKeyFields::Status) return CopyString(L"Select account and confirm on watch", value);
    return E_INVALIDARG;
}
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetBitmapValue(DWORD, HBITMAP* b) { if (b) *b = nullptr; return E_NOTIMPL; }
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetCheckboxValue(DWORD, BOOL*) { return E_NOTIMPL; }
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetComboBoxValueCount(DWORD, DWORD*) { return E_NOTIMPL; }
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetComboBoxValueAt(DWORD, DWORD, PWSTR*) { return E_NOTIMPL; }
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetSubmitButtonValue(DWORD id, DWORD* adjacent) {
    if (!adjacent || id != WristKeyFields::Submit) return E_INVALIDARG;
    *adjacent = WristKeyFields::Status;
    return S_OK;
}
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::SetStringValue(DWORD, PCWSTR) { return E_NOTIMPL; }
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::SetCheckboxValue(DWORD, BOOL) { return E_NOTIMPL; }
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::SetComboBoxSelectedValue(DWORD, DWORD) { return E_NOTIMPL; }
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::CommandLinkClicked(DWORD) { return E_NOTIMPL; }
/**
 * Connects to the daemon's named pipe (see daemon/src/lib.rs, mod pipe_server)
 * and asks it to perform a live BLE challenge-response unlock right now.
 * This blocks for however long that takes — the daemon's own BLE timeout is
 * 10s, so we allow up to 15s here to leave headroom for the round trip.
 *
 * Protocol: write `{"action":"unlock"}\n`, read one line back:
 *   {"status":"success","password":"..."}   -> outPassword set, returns true
 *   {"status":"error","message":"..."}      -> returns false
 *
 * Deliberately does NOT use a JSON library (keeps this DLL's dependency
 * surface minimal) — the response shape is fixed and simple enough that a
 * plain substring search is safe and unambiguous here.
 *
 * UNTESTED — written without a Windows toolchain available to compile or
 * run it. Review the named-pipe error handling and timeout behavior on a
 * real machine before relying on this.
 */
static bool RequestUnlockFromDaemon(std::wstring& outPassword) {
    HANDLE pipe = INVALID_HANDLE_VALUE;
    const wchar_t* pipeName = L"\\\\.\\pipe\\wristkey";

    // The daemon may briefly be busy with another client; wait a couple of
    // seconds for a free pipe instance before giving up.
    if (!WaitNamedPipeW(pipeName, 2000)) {
        return false;
    }
    pipe = CreateFileW(pipeName, GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                        OPEN_EXISTING, 0, nullptr);
    if (pipe == INVALID_HANDLE_VALUE) {
        return false;
    }

    // Message-mode isn't set up server-side (it's a byte-stream pipe), so
    // read in a loop until we see the trailing '\n' the daemon always sends.
    DWORD mode = PIPE_READMODE_BYTE;
    SetNamedPipeHandleState(pipe, &mode, nullptr, nullptr);

    static const char kRequest[] = "{\"action\":\"unlock\"}\n";
    DWORD written = 0;
    if (!WriteFile(pipe, kRequest, static_cast<DWORD>(strlen(kRequest)), &written, nullptr)) {
        CloseHandle(pipe);
        return false;
    }

    std::string response;
    char buf[512];
    const DWORD deadlineMs = GetTickCount() + 15000;
    for (;;) {
        DWORD avail = 0;
        // PeekNamedPipe lets us poll without blocking forever on ReadFile,
        // so we can honor the 15s overall deadline even if the daemon hangs.
        if (PeekNamedPipe(pipe, nullptr, 0, nullptr, &avail, nullptr) && avail == 0) {
            if (GetTickCount() > deadlineMs) { CloseHandle(pipe); return false; }
            Sleep(100);
            continue;
        }
        DWORD read = 0;
        if (!ReadFile(pipe, buf, sizeof(buf) - 1, &read, nullptr) || read == 0) {
            break;
        }
        buf[read] = '\0';
        response.append(buf, read);
        if (response.find('\n') != std::string::npos) break;
        if (GetTickCount() > deadlineMs) break;
    }
    CloseHandle(pipe);

    if (response.find("\"status\":\"success\"") == std::string::npos) {
        return false;
    }
    const std::string key = "\"password\":\"";
    size_t start = response.find(key);
    if (start == std::string::npos) return false;
    start += key.size();
    size_t end = response.find('"', start);
    if (end == std::string::npos) return false;
    std::string password = response.substr(start, end - start);

    // Convert UTF-8 -> UTF-16 for CredPackAuthenticationBufferW.
    int wlen = MultiByteToWideChar(CP_UTF8, 0, password.c_str(), static_cast<int>(password.size()), nullptr, 0);
    if (wlen <= 0) return false;
    outPassword.assign(wlen, L'\0');
    MultiByteToWideChar(CP_UTF8, 0, password.c_str(), static_cast<int>(password.size()), &outPassword[0], wlen);
    return true;
}

/**
 * Packs `username`/`password` into a real Windows credential serialization
 * for CPUS_UNLOCK_WORKSTATION, using CredPackAuthenticationBufferW — the
 * same mechanism Microsoft's own sample Credential Providers use. Looks up
 * the "Negotiate" LSA authentication package, which is what the local
 * unlock/logon path expects.
 *
 * UNTESTED — same caveat as above. This is the standard, well-documented
 * pattern (see Microsoft's SampleCredentialProvider), reproduced from memory
 * without a compiler to check it against.
 */
static HRESULT PackCredential(const std::wstring& username, const std::wstring& password,
                               CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* out) {
    HANDLE lsaHandle = nullptr;
    NTSTATUS status = LsaConnectUntrusted(&lsaHandle);
    if (status != 0) return E_FAIL;

    LSA_STRING name;
    const char* pkg = NEGOSSP_NAME_A; // "Negotiate"
    name.Buffer = const_cast<PCHAR>(pkg);
    name.Length = static_cast<USHORT>(strlen(pkg));
    name.MaximumLength = name.Length + 1;

    ULONG authPackage = 0;
    status = LsaLookupAuthenticationPackage(lsaHandle, &name, &authPackage);
    LsaDeregisterLogonProcess(lsaHandle);
    if (status != 0) return E_FAIL;

    DWORD cbSerialization = 0;
    // First call deliberately fails to report the required buffer size.
    CredPackAuthenticationBufferW(CRED_PACK_PROTECTED_CREDENTIALS,
        const_cast<LPWSTR>(username.c_str()), const_cast<LPWSTR>(password.c_str()),
        nullptr, &cbSerialization);
    if (cbSerialization == 0) return E_FAIL;

    BYTE* rgbSerialization = static_cast<BYTE*>(CoTaskMemAlloc(cbSerialization));
    if (!rgbSerialization) return E_OUTOFMEMORY;

    if (!CredPackAuthenticationBufferW(CRED_PACK_PROTECTED_CREDENTIALS,
            const_cast<LPWSTR>(username.c_str()), const_cast<LPWSTR>(password.c_str()),
            rgbSerialization, &cbSerialization)) {
        CoTaskMemFree(rgbSerialization);
        return E_FAIL;
    }

    out->ulAuthenticationPackage = authPackage;
    out->clsidCredentialProvider = CLSID_WristKeyCredentialProvider;
    out->cbSerialization = cbSerialization;
    out->rgbSerialization = rgbSerialization;
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetSerialization(
    CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE* response,
    CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* serialization, PWSTR* status, CREDENTIAL_PROVIDER_STATUS_ICON* icon) {
    if (!response || !serialization || !status || !icon) return E_POINTER;
    ZeroMemory(serialization, sizeof(*serialization));
    *status = nullptr;
    *icon = CPSI_NONE;

    std::wstring password;
    if (RequestUnlockFromDaemon(password)) {
        if (SUCCEEDED(PackCredential(_username, password, serialization))) {
            *response = CPGSR_RETURN_CREDENTIAL_FINISHED;
            *icon = CPSI_SUCCESS;
            return S_OK;
        }
        // Password was retrieved but packing failed (LSA lookup or
        // CredPackAuthenticationBufferW error) — fall through to the
        // not-finished path below rather than risk a half-built
        // serialization reaching Winlogon.
    }

    // Watch didn't confirm in time, daemon isn't running, or packing
    // failed — tell Windows we have nothing to submit yet. The person
    // just sees the tile stay put; they can try again or use another
    // sign-in option. Never silently claim success here.
    *response = CPGSR_NO_CREDENTIAL_NOT_FINISHED;
    return S_OK;
}
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::ReportResult(NTSTATUS, NTSTATUS, PWSTR* status, CREDENTIAL_PROVIDER_STATUS_ICON* icon) {
    if (status) *status = nullptr;
    if (icon) *icon = CPSI_NONE;
    return S_OK;
}
