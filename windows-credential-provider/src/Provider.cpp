#include "Provider.h"
#include <new>
#include <shlwapi.h>
#include <lm.h>
#include <string>

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
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetSerialization(CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE* response,
    CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* serialization, PWSTR* status, CREDENTIAL_PROVIDER_STATUS_ICON* icon) {
    if (!response || !serialization || !status || !icon) return E_POINTER;
    *response = CPGSR_NO_CREDENTIAL_NOT_FINISHED;
    ZeroMemory(serialization, sizeof(*serialization));
    *status = nullptr;
    *icon = CPSI_NONE;
    return S_OK;
}
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::ReportResult(NTSTATUS, NTSTATUS, PWSTR* status, CREDENTIAL_PROVIDER_STATUS_ICON* icon) {
    if (status) *status = nullptr;
    if (icon) *icon = CPSI_NONE;
    return S_OK;
}
