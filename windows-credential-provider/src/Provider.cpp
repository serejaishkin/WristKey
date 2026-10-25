#include "Provider.h"
#include "helpers.h"

#include <cstdio>
#include <windows.h>
#include <shlwapi.h>
#include <cctype>
#include <string>
#include <vector>
#include <wchar.h>

#pragma comment(lib, "advapi32.lib")
#pragma comment(lib, "credui.lib")
#pragma comment(lib, "secur32.lib")
#pragma comment(lib, "shlwapi.lib")
#pragma comment(lib, "kernel32.lib")
#pragma comment(lib, "ole32.lib")
#pragma comment(lib, "oleaut32.lib")
#pragma comment(lib, "uuid.lib")

#ifndef CPFG_CREDENTIAL_PROVIDER_LOGO
#define CPFG_CREDENTIAL_PROVIDER_LOGO GUID{0x2d837775, 0xf6cd, 0x464e, {0xa7, 0x45, 0x48, 0x2f, 0xd0, 0xb4, 0x74, 0x93}}
#endif

// CLSID_WristKeyCredentialProvider declared in Guid.cpp
extern "C" const GUID CLSID_WristKeyCredentialProvider;

// PKEY_Identity_QualifiedUserName provided by libuuid
extern const PROPERTYKEY PKEY_Identity_QualifiedUserName;

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
// Helpers
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

static bool ExtractJsonStringValue(const std::string& text, const std::string& key, std::string& out) {
    const std::string needle = "\"" + key + "\":";
    const size_t keyPos = text.find(needle);
    if (keyPos == std::string::npos) return false;

    size_t p = keyPos + needle.size();
    while (p < text.size() && std::isspace(static_cast<unsigned char>(text[p]))) ++p;
    if (p >= text.size() || text[p] != '"') return false;

    ++p;
    out.clear();
    while (p < text.size()) {
        const char c = text[p];
        if (c == '"') return true;
        if (c != '\\') { out += c; ++p; continue; }
        ++p;
        if (p >= text.size()) return false;
        switch (text[p]) {
        case '"':  out += '"';  ++p; break;
        case '\\': out += '\\'; ++p; break;
        case '/':  out += '/';  ++p; break;
        case 'b':  out += '\b'; ++p; break;
        case 'f':  out += '\f'; ++p; break;
        case 'n':  out += '\n'; ++p; break;
        case 'r':  out += '\r'; ++p; break;
        case 't':  out += '\t'; ++p; break;
        case 'u': {
            unsigned cp = 0;
            for (int i = 0; i < 4; ++i) {
                ++p;
                if (p >= text.size()) return false;
                const char h = text[p];
                cp <<= 4;
                if (h >= '0' && h <= '9') cp |= static_cast<unsigned>(h - '0');
                else if (h >= 'a' && h <= 'f') cp |= static_cast<unsigned>(h - 'a' + 10);
                else if (h >= 'A' && h <= 'F') cp |= static_cast<unsigned>(h - 'A' + 10);
                else return false;
            }
            if (cp >= 0xD800 && cp <= 0xDBFF && p + 6 < text.size() &&
                text[p + 1] == '\\' && text[p + 2] == 'u') {
                p += 2;
                unsigned low = 0;
                for (int i = 0; i < 4; ++i) {
                    ++p;
                    if (p >= text.size()) return false;
                    const char h = text[p];
                    low <<= 4;
                    if (h >= '0' && h <= '9') low |= static_cast<unsigned>(h - '0');
                    else if (h >= 'a' && h <= 'f') low |= static_cast<unsigned>(h - 'a' + 10);
                    else if (h >= 'A' && h <= 'F') low |= static_cast<unsigned>(h - 'A' + 10);
                    else return false;
                }
                if (low >= 0xDC00 && low <= 0xDFFF) {
                    cp = 0x10000 + ((cp - 0xD800) << 10) + (low - 0xDC00);
                }
            }
            if (cp < 0x80) {
                out += static_cast<char>(cp);
            } else if (cp < 0x800) {
                out += static_cast<char>(0xC0 | (cp >> 6));
                out += static_cast<char>(0x80 | (cp & 0x3F));
            } else if (cp < 0x10000) {
                out += static_cast<char>(0xE0 | (cp >> 12));
                out += static_cast<char>(0x80 | ((cp >> 6) & 0x3F));
                out += static_cast<char>(0x80 | (cp & 0x3F));
            } else {
                out += static_cast<char>(0xF0 | (cp >> 18));
                out += static_cast<char>(0x80 | ((cp >> 12) & 0x3F));
                out += static_cast<char>(0x80 | ((cp >> 6) & 0x3F));
                out += static_cast<char>(0x80 | (cp & 0x3F));
            }
            ++p;
            break;
        }
        default:
            return false;
        }
    }
    return false;
}

static bool RequestUnlockFromDaemon(std::wstring& outPassword, std::wstring& outError,
                                     ICredentialProviderCredentialEvents* events = nullptr,
                                     ICredentialProviderCredential* credential = nullptr) {
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
    const DWORD startTick = GetTickCount();
    const DWORD totalWaitMs = 25000;
    const DWORD deadline = startTick + totalWaitMs;
    bool timedOut = false;
    DWORD lastStatusUpdateTick = 0;

    for (;;) {
        DWORD available = 0;
        if (PeekNamedPipe(pipe, nullptr, 0, nullptr, &available, nullptr) && available == 0) {
            if (GetTickCount() > deadline) {
                timedOut = true;
                break;
            }

            if (events && credential) {
                const DWORD now = GetTickCount();
                if (now - lastStatusUpdateTick >= 1000) {
                    lastStatusUpdateTick = now;
                    const DWORD elapsedMs = now - startTick;
                    const DWORD remainingSeconds =
                        elapsedMs < totalWaitMs ? (totalWaitMs - elapsedMs + 999) / 1000 : 0;
                    const std::wstring statusText =
                        L"Confirm on your watch\u2026 (" + std::to_wstring(remainingSeconds) + L"s)";
                    events->SetFieldString(credential, WristKeyFields::Status, statusText.c_str());
                }
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

    if (events && credential) {
        events->SetFieldString(credential, WristKeyFields::Status, L"Confirm on your watch");
    }

    if (timedOut && response.empty()) {
        outError = L"WristKey: daemon did not answer.";
        return false;
    }

    std::string status;
    if (!ExtractJsonStringValue(response, "status", status) || status != "success") {
        std::string message;
        if (ExtractJsonStringValue(response, "message", message) && !message.empty()) {
            outError = L"WristKey: " + Utf8ToWide(message) + L".";
        } else {
            outError = L"WristKey: the watch did not confirm the unlock.";
        }
        return false;
    }

    std::string password;
    if (!ExtractJsonStringValue(response, "password", password) || password.empty()) {
        outError = L"WristKey: daemon returned a malformed password.";
        return false;
    }

    outPassword = Utf8ToWide(password);
    return true;
}

static bool QueryExpectedUsernameFromDaemon(std::wstring& outUsername) {
    outUsername.clear();
    const wchar_t* pipeName = L"\\\\.\\pipe\\wristkey";

    if (!WaitNamedPipeW(pipeName, 500)) return false;

    HANDLE pipe = CreateFileW(pipeName, GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                              OPEN_EXISTING, 0, nullptr);
    if (pipe == INVALID_HANDLE_VALUE) return false;

    DWORD mode = PIPE_READMODE_BYTE;
    SetNamedPipeHandleState(pipe, &mode, nullptr, nullptr);

    static const char request[] = "{\"action\":\"get_username\"}\n";
    DWORD written = 0;
    if (!WriteFile(pipe, request, static_cast<DWORD>(strlen(request)), &written, nullptr)) {
        CloseHandle(pipe);
        return false;
    }

    std::string response;
    char buffer[512];
    const DWORD deadline = GetTickCount() + 1000;
    bool timedOut = false;

    for (;;) {
        DWORD available = 0;
        if (PeekNamedPipe(pipe, nullptr, 0, nullptr, &available, nullptr) && available == 0) {
            if (GetTickCount() > deadline) { timedOut = true; break; }
            Sleep(25);
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

    if (timedOut && response.empty()) return false;

    std::string status;
    if (!ExtractJsonStringValue(response, "status", status) || status != "success") return false;

    std::string username;
    if (!ExtractJsonStringValue(response, "username", username) || username.empty()) return false;

    outUsername = Utf8ToWide(username);
    return !outUsername.empty();
}

static bool QueryWatchPresentFromDaemon() {
    const wchar_t* pipeName = L"\\\\.\\pipe\\wristkey";

    if (!WaitNamedPipeW(pipeName, 300)) return false;

    HANDLE pipe = CreateFileW(pipeName, GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                              OPEN_EXISTING, 0, nullptr);
    if (pipe == INVALID_HANDLE_VALUE) return false;

    DWORD mode = PIPE_READMODE_BYTE;
    SetNamedPipeHandleState(pipe, &mode, nullptr, nullptr);

    static const char request[] = "{\"action\":\"proximity\"}\n";
    DWORD written = 0;
    if (!WriteFile(pipe, request, static_cast<DWORD>(strlen(request)), &written, nullptr)) {
        CloseHandle(pipe);
        return false;
    }

    std::string response;
    char buffer[256];
    const DWORD deadline = GetTickCount() + 700;
    for (;;) {
        DWORD available = 0;
        if (PeekNamedPipe(pipe, nullptr, 0, nullptr, &available, nullptr) && available == 0) {
            if (GetTickCount() > deadline) break;
            Sleep(20);
            continue;
        }
        DWORD read = 0;
        if (!ReadFile(pipe, buffer, sizeof(buffer) - 1, &read, nullptr) || read == 0) break;
        response.append(buffer, read);
        if (response.find('\n') != std::string::npos) break;
        if (GetTickCount() > deadline) break;
    }
    CloseHandle(pipe);

    return response.find("\"status\":\"success\"") != std::string::npos
        && response.find("\"present\":true") != std::string::npos;
}

// Serialize password as: domain\0user\0password\0 (UTF-16)
static HRESULT PackUnlockCredential(
    const std::wstring& qualifiedUsername,
    const std::wstring& password,
    CREDENTIAL_PROVIDER_USAGE_SCENARIO scenario,
    CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* out) {
    if (!out) return E_POINTER;
    ZeroMemory(out, sizeof(*out));

    if (qualifiedUsername.empty()) return E_INVALIDARG;

    size_t domainLen = 0;
    size_t userLen = qualifiedUsername.length();
    size_t passLen = password.length();

    size_t userPos = domainLen;
    size_t passPos = userPos + userLen + 1;
    size_t totalSize = passPos + passLen + 1;

    size_t totalBytes = totalSize * sizeof(wchar_t);

    PBYTE rgbSerialization = static_cast<PBYTE>(CoTaskMemAlloc(totalBytes));
    if (!rgbSerialization) return E_OUTOFMEMORY;

    PWSTR p = reinterpret_cast<PWSTR>(rgbSerialization);

    // Domain (empty)
    *p++ = L'\0';

    // Username
    memcpy(p, qualifiedUsername.c_str(), userLen * sizeof(wchar_t));
    p += userLen;
    *p++ = L'\0';

    // Password
    memcpy(p, password.c_str(), passLen * sizeof(wchar_t));
    p += passLen;
    *p++ = L'\0';

    out->rgbSerialization = rgbSerialization;
    out->cbSerialization = static_cast<ULONG>(totalBytes);
    out->ulAuthenticationPackage = 0;
    out->clsidCredentialProvider = CLSID_WristKeyCredentialProvider;

    return S_OK;
}

static std::wstring WinErrorText(DWORD error) {
    wchar_t message[512]{};
    const DWORD length = FormatMessageW(
        FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
        nullptr,
        error,
        0,
        message,
        ARRAYSIZE(message),
        nullptr);

    if (length == 0) {
        return L"Windows error " + std::to_wstring(error);
    }

    std::wstring text(message, length);
    while (!text.empty() && (text.back() == L'\r' || text.back() == L'\n' || text.back() == L' ')) {
        text.pop_back();
    }
    return L"Windows: " + text + L" (error " + std::to_wstring(error) + L")";
}

// -----------------------------------------------------------------------------
// WristKeyProvider
// -----------------------------------------------------------------------------

WristKeyProvider::WristKeyProvider() = default;

WristKeyProvider::~WristKeyProvider() {
    ReleaseCredentials();
    if (_userArray) { _userArray->Release(); _userArray = nullptr; }
    if (_events) { _events->Release(); _events = nullptr; }
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
        return CopyString(L"WristKey", &d.pszLabel)
            ? [&]() -> HRESULT {
                auto* copy = static_cast<CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR*>(CoTaskMemAlloc(sizeof(d)));
                if (!copy) { CoTaskMemFree(d.pszLabel); return E_OUTOFMEMORY; }
                *copy = d; *out = copy; return S_OK;
              }() : E_OUTOFMEMORY;
    case WristKeyFields::Tile:
        d.cpft = CPFT_LARGE_TEXT;
        return CopyString(L"WristKey", &d.pszLabel)
            ? [&]() -> HRESULT {
                auto* copy = static_cast<CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR*>(CoTaskMemAlloc(sizeof(d)));
                if (!copy) { CoTaskMemFree(d.pszLabel); return E_OUTOFMEMORY; }
                *copy = d; *out = copy; return S_OK;
              }() : E_OUTOFMEMORY;
    case WristKeyFields::Status:
        d.cpft = CPFT_SMALL_TEXT;
        return CopyString(L"Confirm on your watch", &d.pszLabel)
            ? [&]() -> HRESULT {
                auto* copy = static_cast<CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR*>(CoTaskMemAlloc(sizeof(d)));
                if (!copy) { CoTaskMemFree(d.pszLabel); return E_OUTOFMEMORY; }
                *copy = d; *out = copy; return S_OK;
              }() : E_OUTOFMEMORY;
    case WristKeyFields::Submit:
        d.cpft = CPFT_SUBMIT_BUTTON;
        return CopyString(L"Unlock", &d.pszLabel)
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
    if (_userArray) { _userArray->Release(); _userArray = nullptr; }

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

    if ((_scenario != CPUS_LOGON && _scenario != CPUS_UNLOCK_WORKSTATION) || !_userArray) {
        return S_OK;
    }

    DWORD userCount = 0;
    HRESULT hr = _userArray->GetCount(&userCount);
    if (FAILED(hr)) return hr;

    DWORD selectedIndex = 0;
    if (userCount > 1) {
        std::wstring expectedUsername;
        if (QueryExpectedUsernameFromDaemon(expectedUsername) && !expectedUsername.empty()) {
            for (DWORD i = 0; i < userCount; ++i) {
                ICredentialProviderUser* candidate = nullptr;
                if (FAILED(_userArray->GetAt(i, &candidate)) || !candidate) continue;
                PWSTR candidateName = nullptr;
                const bool matched = SUCCEEDED(candidate->GetStringValue(PKEY_Identity_QualifiedUserName, &candidateName))
                    && candidateName
                    && _wcsicmp(candidateName, expectedUsername.c_str()) == 0;
                CoTaskMemFree(candidateName);
                candidate->Release();
                if (matched) {
                    selectedIndex = i;
                    break;
                }
            }
        }
    }

    if (userCount > 0) {
        ICredentialProviderUser* selectedUser = nullptr;
        if (SUCCEEDED(_userArray->GetAt(selectedIndex, &selectedUser)) && selectedUser) {
            PWSTR username = nullptr;
            PWSTR sid = nullptr;

            HRESULT userHr = selectedUser->GetStringValue(PKEY_Identity_QualifiedUserName, &username);
            if (SUCCEEDED(userHr)) {
                userHr = selectedUser->GetSid(&sid);
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
            selectedUser->Release();
        }
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

    if (!_credentials.empty()) {
        const DWORD now = GetTickCount();
        const bool cooledDown = _lastAutoLogonTick == 0 || (now - _lastAutoLogonTick) > 30000;
        if (cooledDown && QueryWatchPresentFromDaemon()) {
            _lastAutoLogonTick = now ? now : 1;
            *def = 0;
            *autoLogon = TRUE;
        }
    }
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
    if (_events) { _events->Release(); _events = nullptr; }
    if (_provider) { _provider->Release(); _provider = nullptr; }
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
    if (!n) delete this;
    return n;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::Advise(ICredentialProviderCredentialEvents* e) {
    if (_events) { _events->Release(); _events = nullptr; }
    _events = e;
    if (_events) _events->AddRef();
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::UnAdvise() {
    if (_events) { _events->Release(); _events = nullptr; }
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
    if (!RequestUnlockFromDaemon(password, daemonError, _events,
                                  static_cast<ICredentialProviderCredential*>(this))) {
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

    if (ntsStatus == 0 && ntsSubstatus == 0) {
        return S_OK;
    }

    const NTSTATUS detailStatus = ntsSubstatus != 0 ? ntsSubstatus : ntsStatus;
    const DWORD winError = LsaNtStatusToWinError(detailStatus);

    if (status) {
        const std::wstring text = WinErrorText(winError);
        CopyString((L"WristKey: " + text).c_str(), status);
    }
    if (icon) *icon = CPSI_ERROR;
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetUserSid(PWSTR* sid) {
    if (!sid) return E_POINTER;
    *sid = nullptr;
    if (_sid.empty()) return E_FAIL;
    return CopyString(_sid.c_str(), sid);
}
