#include "Provider.h"
#include <new>
#include <shlwapi.h>

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

WristKeyProvider::WristKeyProvider() {
    _credential = new (std::nothrow) WristKeyProviderCredential(this);
}

WristKeyProvider::~WristKeyProvider() {
    if (_events) _events->Release();
    if (_credential) _credential->Release();
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
                                    index == WristKeyFields::Status ? L"Waiting for watch..." : L"Unlock");
    return SHCreateCredentialProviderFieldDescriptor(&d, out);
}
HRESULT STDMETHODCALLTYPE WristKeyProvider::GetCredentialCount(DWORD* count, DWORD* def, BOOL* autoLogon) {
    if (!count || !def || !autoLogon) return E_POINTER;
    *count = 1; *def = CREDENTIAL_PROVIDER_NO_DEFAULT; *autoLogon = FALSE;
    return S_OK;
}
HRESULT STDMETHODCALLTYPE WristKeyProvider::GetCredentialAt(DWORD index, ICredentialProviderCredential** out) {
    if (!out) return E_POINTER;
    *out = nullptr;
    if (index != 0 || !_credential) return E_INVALIDARG;
    _credential->AddRef();
    *out = _credential;
    return S_OK;
}
void WristKeyProvider::SetEvents(ICredentialProviderCredentialEvents*, UINT_PTR) {}
bool WristKeyProvider::WatchAvailable() const { return false; }
void WristKeyProvider::RefreshStatus() {}

WristKeyProviderCredential::WristKeyProviderCredential(WristKeyProvider* provider) : _provider(provider) {
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
    if (id == WristKeyFields::Tile) return CopyString(L"WristKey", value);
    if (id == WristKeyFields::Status) return CopyString(L"Waiting for watch...", value);
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
