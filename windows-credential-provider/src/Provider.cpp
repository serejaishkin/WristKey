// *** REFERENCE IMPLEMENTATION -- see AI_HANDOFF.md ("ЭТАЛОН"). Do not rewrite;
// minimal diffs only, and only when the owner asks. Restore: git checkout 32ca1d2 -- windows-credential-provider/ ***
// WristKey Credential Provider (V2): one tile per sign-in user, unlocks by
// asking the WristKey service (\\.\pipe\wristkey) for a watch-confirmed password.
#include "Provider.h"
#include "ServiceInstall.h"

#include <windows.h>
#include <ntsecapi.h>
#include <shlwapi.h>
#include <wchar.h>

#include <cctype>
#include <cstdio>
#include <functional>
#include <new>
#include <string>
#include <vector>

#ifdef _MSC_VER
#pragma comment(lib, "advapi32.lib")
#pragma comment(lib, "secur32.lib")
#pragma comment(lib, "shlwapi.lib")
#pragma comment(lib, "ole32.lib")
#pragma comment(lib, "uuid.lib")
#endif

#ifndef CPFG_CREDENTIAL_PROVIDER_LOGO
#define CPFG_CREDENTIAL_PROVIDER_LOGO GUID{0x2d837775, 0xf6cd, 0x464e, {0xa7, 0x45, 0x48, 0x2f, 0xd0, 0xb4, 0x74, 0x93}}
#endif

extern "C" const GUID CLSID_WristKeyCredentialProvider;
// PKEY_Identity_QualifiedUserName, defined locally: MSVC's uuid.lib does not export it.
static const PROPERTYKEY PKEY_Identity_QualifiedUserName =
    {{0xda520e51, 0xf4e9, 0x4739, {0xac, 0x82, 0x02, 0xe0, 0xa9, 0x5c, 0x90, 0x30}}, 100};

static const wchar_t kPipeName[] = L"\\\\.\\pipe\\wristkey";
static const DWORD kUnlockWaitMs = 25000;     // watch confirmation budget
static const DWORD kAutoLogonCooldownMs = 30000;
static const DWORD kPollIntervalMs = 1500;

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
// JSON / text helpers
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
// Daemon pipe client
// -----------------------------------------------------------------------------

// Sends one JSON line, reads one JSON line. `onWait(elapsedMs)` is called about
// every 100 ms while waiting. Returns false on connect/IO failure or timeout.
static bool PipeRoundTrip(const char* request, DWORD connectMs, DWORD totalMs,
                          std::string& response, std::wstring* error,
                          const std::function<void(DWORD)>& onWait = nullptr) {
    response.clear();
    if (!WaitNamedPipeW(kPipeName, connectMs)) {
        if (error) *error = L"WristKey service is not running.";
        return false;
    }
    HANDLE pipe = CreateFileW(kPipeName, GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                              OPEN_EXISTING, 0, nullptr);
    if (pipe == INVALID_HANDLE_VALUE) {
        if (error) *error = L"WristKey: cannot connect to the service.";
        return false;
    }
    DWORD mode = PIPE_READMODE_BYTE;
    SetNamedPipeHandleState(pipe, &mode, nullptr, nullptr);

    DWORD written = 0;
    if (!WriteFile(pipe, request, static_cast<DWORD>(strlen(request)), &written, nullptr)) {
        CloseHandle(pipe);
        if (error) *error = L"WristKey: service communication failed.";
        return false;
    }

    const DWORD start = GetTickCount();
    char buffer[512];
    bool gotLine = false;
    for (;;) {
        const DWORD elapsed = GetTickCount() - start;
        DWORD available = 0;
        if (!PeekNamedPipe(pipe, nullptr, 0, nullptr, &available, nullptr)) break;  // pipe closed
        if (available == 0) {
            if (elapsed >= totalMs) break;
            if (onWait) onWait(elapsed);
            Sleep(60);
            continue;
        }
        DWORD read = 0;
        if (!ReadFile(pipe, buffer, sizeof(buffer), &read, nullptr) || read == 0) break;
        response.append(buffer, read);
        if (response.find('\n') != std::string::npos) { gotLine = true; break; }
    }
    CloseHandle(pipe);

    if (!gotLine && response.empty()) {
        if (error) *error = L"WristKey: the service did not answer.";
        return false;
    }
    return true;
}

static bool RequestUnlockFromDaemon(std::wstring& outPassword, std::wstring& outError,
                                    ICredentialProviderCredentialEvents* events,
                                    ICredentialProviderCredential* credential) {
    DWORD lastShown = 0xFFFFFFFF;
    std::string response;
    const bool ok = PipeRoundTrip(
        "{\"action\":\"unlock\"}\n", 2000, kUnlockWaitMs, response, &outError,
        [&](DWORD elapsed) {
            if (!events || !credential) return;
            const DWORD remaining = elapsed < kUnlockWaitMs ? (kUnlockWaitMs - elapsed + 999) / 1000 : 0;
            if (remaining == lastShown) return;
            lastShown = remaining;
            const std::wstring text = L"Confirm on your watch… (" + std::to_wstring(remaining) + L"s)";
            events->SetFieldString(credential, WristKeyFields::Status, text.c_str());
        });
    if (events && credential) {
        events->SetFieldString(credential, WristKeyFields::Status, L"Confirm on your watch");
    }
    if (!ok) return false;

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
        outError = L"WristKey: the service returned no password (set it in the WristKey app).";
        return false;
    }
    outPassword = Utf8ToWide(password);
    if (!password.empty()) SecureZeroMemory(&password[0], password.size());
    return true;
}

static bool QueryExpectedUsernameFromDaemon(std::wstring& outUsername) {
    outUsername.clear();
    std::string response;
    if (!PipeRoundTrip("{\"action\":\"get_username\"}\n", 500, 1000, response, nullptr)) return false;
    std::string status, username;
    if (!ExtractJsonStringValue(response, "status", status) || status != "success") return false;
    if (!ExtractJsonStringValue(response, "username", username) || username.empty()) return false;
    outUsername = Utf8ToWide(username);
    return !outUsername.empty();
}

static bool QueryWatchPresentFromDaemon() {
    std::string response;
    if (!PipeRoundTrip("{\"action\":\"proximity\"}\n", 300, 700, response, nullptr)) return false;
    return response.find("\"status\":\"success\"") != std::string::npos
        && response.find("\"present\":true") != std::string::npos;
}

// -----------------------------------------------------------------------------
// Serialization: KERB_INTERACTIVE_UNLOCK_LOGON in the self-relative form that
// LogonUI hands to LSA (UNICODE_STRING.Buffer holds an offset).
// -----------------------------------------------------------------------------

static void SplitQualifiedName(const std::wstring& qualified, std::wstring& domain, std::wstring& user) {
    const size_t slash = qualified.find(L'\\');
    if (slash != std::wstring::npos) {
        domain = qualified.substr(0, slash);
        user = qualified.substr(slash + 1);
        if (domain == L".") domain.clear();
    } else {
        domain.clear();
        user = qualified;
    }
    if (domain.empty() && user.find(L'@') == std::wstring::npos) {
        wchar_t computer[MAX_COMPUTERNAME_LENGTH + 1]{};
        DWORD n = ARRAYSIZE(computer);
        if (GetComputerNameW(computer, &n)) domain = computer;
    }
}

static HRESULT AuthenticationPackageId(ULONG* id) {
    HANDLE lsa = nullptr;
    NTSTATUS st = LsaConnectUntrusted(&lsa);
    if (st != 0) return HRESULT_FROM_WIN32(LsaNtStatusToWinError(st));
    char name[] = "Negotiate";
    LSA_STRING pkg{};
    pkg.Buffer = name;
    pkg.Length = static_cast<USHORT>(strlen(name));
    pkg.MaximumLength = static_cast<USHORT>(sizeof(name));
    st = LsaLookupAuthenticationPackage(lsa, &pkg, id);
    LsaDeregisterLogonProcess(lsa);
    return st == 0 ? S_OK : HRESULT_FROM_WIN32(LsaNtStatusToWinError(st));
}

static HRESULT PackUnlockCredential(const std::wstring& qualifiedUsername, const std::wstring& password,
                                    CREDENTIAL_PROVIDER_USAGE_SCENARIO scenario,
                                    CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* out) {
    if (!out) return E_POINTER;
    ZeroMemory(out, sizeof(*out));
    if (qualifiedUsername.empty() || password.empty()) return E_INVALIDARG;

    std::wstring domain, user;
    SplitQualifiedName(qualifiedUsername, domain, user);
    if (user.empty()) return E_INVALIDARG;

    const size_t strBytes = (domain.size() + user.size() + password.size()) * sizeof(wchar_t);
    const size_t total = sizeof(KERB_INTERACTIVE_UNLOCK_LOGON) + strBytes;
    if (total > 0xFFFF0000u) return E_INVALIDARG;
    auto* buf = static_cast<BYTE*>(CoTaskMemAlloc(total));
    if (!buf) return E_OUTOFMEMORY;
    ZeroMemory(buf, total);

    auto* kiul = reinterpret_cast<KERB_INTERACTIVE_UNLOCK_LOGON*>(buf);
    KERB_INTERACTIVE_LOGON& kil = kiul->Logon;
    kil.MessageType = (scenario == CPUS_UNLOCK_WORKSTATION) ? KerbWorkstationUnlockLogon : KerbInteractiveLogon;

    size_t offset = sizeof(KERB_INTERACTIVE_UNLOCK_LOGON);
    auto put = [&](UNICODE_STRING& us, const std::wstring& s) {
        us.Length = static_cast<USHORT>(s.size() * sizeof(wchar_t));
        us.MaximumLength = us.Length;
        if (!s.empty()) memcpy(buf + offset, s.data(), us.Length);
        us.Buffer = reinterpret_cast<PWSTR>(static_cast<ULONG_PTR>(offset));  // self-relative
        offset += us.Length;
    };
    put(kil.LogonDomainName, domain);
    put(kil.UserName, user);
    put(kil.Password, password);

    ULONG pkgId = 0;
    HRESULT hr = AuthenticationPackageId(&pkgId);
    if (FAILED(hr)) {
        SecureZeroMemory(buf, total);
        CoTaskMemFree(buf);
        return hr;
    }

    out->rgbSerialization = buf;
    out->cbSerialization = static_cast<ULONG>(total);
    out->ulAuthenticationPackage = pkgId;
    out->clsidCredentialProvider = CLSID_WristKeyCredentialProvider;
    return S_OK;
}

// -----------------------------------------------------------------------------
// WristKeyProvider
// -----------------------------------------------------------------------------

WristKeyProvider::WristKeyProvider() = default;

WristKeyProvider::~WristKeyProvider() {
    StopPresenceWatcher();
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

ULONG STDMETHODCALLTYPE WristKeyProvider::AddRef() { return InterlockedIncrement(&_ref); }

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

// --- presence watcher --------------------------------------------------------
// While the sign-in UI is shown, poll the service; when the watch comes into
// range, ask LogonUI to re-query GetCredentialCount (which then requests
// auto-logon). Fires only on the absent -> present edge.

DWORD WINAPI WristKeyProvider::PresenceThreadProc(LPVOID param) {
    auto* self = static_cast<WristKeyProvider*>(param);
    bool wasPresent = false;
    DWORD lastEnsure = 0;
    do {
        // Service down (never started / crashed / not installed yet): start or
        // install it from HKLM\SOFTWARE\WristKey\ExePath. Retry every 20 s.
        if (!WaitNamedPipeW(kPipeName, 0) && GetLastError() == ERROR_FILE_NOT_FOUND) {
            const DWORD now = GetTickCount();
            if (lastEnsure == 0 || now - lastEnsure > 20000) {
                lastEnsure = now ? now : 1;
                EnsureWristKeyServiceRunning(std::wstring());
            }
        }
        if (!self->_haveCredentials) continue;
        const bool present = QueryWatchPresentFromDaemon();
        if (present && !wasPresent) {
            const DWORD last = self->_lastAutoLogonTick;
            const DWORD now = GetTickCount();
            if (last == 0 || now - last > kAutoLogonCooldownMs) {
                self->RefreshStatus();
            }
        }
        wasPresent = present;
    } while (WaitForSingleObject(self->_stopEvent, kPollIntervalMs) == WAIT_TIMEOUT);
    return 0;
}

void WristKeyProvider::StartPresenceWatcher() {
    if (_thread) return;
    _stopEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    if (!_stopEvent) return;
    _thread = CreateThread(nullptr, 0, PresenceThreadProc, this, 0, nullptr);
    if (!_thread) { CloseHandle(_stopEvent); _stopEvent = nullptr; }
}

void WristKeyProvider::StopPresenceWatcher() {
    if (_thread) {
        SetEvent(_stopEvent);
        WaitForSingleObject(_thread, 5000);
        CloseHandle(_thread);
        _thread = nullptr;
    }
    if (_stopEvent) { CloseHandle(_stopEvent); _stopEvent = nullptr; }
}

HRESULT STDMETHODCALLTYPE WristKeyProvider::Advise(ICredentialProviderEvents* pcpe, UINT_PTR context) {
    StopPresenceWatcher();
    if (_events) _events->Release();
    _events = pcpe;
    _adviseContext = context;
    if (_events) {
        _events->AddRef();
        StartPresenceWatcher();
    }
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProvider::UnAdvise() {
    StopPresenceWatcher();  // join before dropping the events pointer
    if (_events) { _events->Release(); _events = nullptr; }
    return S_OK;
}

// --- fields ------------------------------------------------------------------

HRESULT STDMETHODCALLTYPE WristKeyProvider::GetFieldDescriptorCount(DWORD* count) {
    if (!count) return E_POINTER;
    *count = WristKeyFields::Count;
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProvider::GetFieldDescriptorAt(DWORD index, CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR** out) {
    if (!out) return E_POINTER;
    *out = nullptr;

    CREDENTIAL_PROVIDER_FIELD_TYPE type;
    GUID guid = GUID_NULL;
    PCWSTR label;
    switch (index) {
    case WristKeyFields::TileImage: type = CPFT_TILE_IMAGE;    guid = CPFG_CREDENTIAL_PROVIDER_LOGO; label = L"WristKey"; break;
    case WristKeyFields::Tile:      type = CPFT_LARGE_TEXT;    label = L"WristKey"; break;
    case WristKeyFields::Status:    type = CPFT_SMALL_TEXT;    label = L"Status"; break;
    case WristKeyFields::Submit:    type = CPFT_SUBMIT_BUTTON; label = L"Unlock"; break;
    default: return E_INVALIDARG;
    }

    auto* d = static_cast<CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR*>(CoTaskMemAlloc(sizeof(CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR)));
    if (!d) return E_OUTOFMEMORY;
    ZeroMemory(d, sizeof(CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR));
    d->dwFieldID = index;
    d->cpft = type;
    d->guidFieldType = guid;
    const HRESULT hr = CopyString(label, &d->pszLabel);
    if (FAILED(hr)) { CoTaskMemFree(d); return hr; }
    *out = d;
    return S_OK;
}

// --- users / credentials -----------------------------------------------------

HRESULT STDMETHODCALLTYPE WristKeyProvider::SetUserArray(ICredentialProviderUserArray* users) {
    if (_userArray) { _userArray->Release(); _userArray = nullptr; }
    _userArray = users;
    if (_userArray) _userArray->AddRef();
    _recreateCredentials = true;
    return S_OK;
}

void WristKeyProvider::ReleaseCredentials() {
    _haveCredentials = false;
    for (auto* credential : _credentials) {
        if (credential) credential->Release();
    }
    _credentials.clear();
}

HRESULT WristKeyProvider::CreateCredentials() {
    ReleaseCredentials();
    if ((_scenario != CPUS_LOGON && _scenario != CPUS_UNLOCK_WORKSTATION) || !_userArray) return S_OK;

    DWORD userCount = 0;
    HRESULT hr = _userArray->GetCount(&userCount);
    if (FAILED(hr) || userCount == 0) return FAILED(hr) ? hr : S_OK;

    // Prefer the account the watch was set up for; otherwise the first user.
    DWORD selected = 0;
    std::wstring expected;
    if (userCount > 1 && QueryExpectedUsernameFromDaemon(expected)) {
        for (DWORD i = 0; i < userCount; ++i) {
            ICredentialProviderUser* candidate = nullptr;
            if (FAILED(_userArray->GetAt(i, &candidate)) || !candidate) continue;
            PWSTR name = nullptr;
            const bool match = SUCCEEDED(candidate->GetStringValue(PKEY_Identity_QualifiedUserName, &name))
                && name && _wcsicmp(name, expected.c_str()) == 0;
            CoTaskMemFree(name);
            candidate->Release();
            if (match) { selected = i; break; }
        }
    }

    ICredentialProviderUser* user = nullptr;
    if (FAILED(_userArray->GetAt(selected, &user)) || !user) return S_OK;

    PWSTR name = nullptr;
    PWSTR sid = nullptr;
    if (SUCCEEDED(user->GetStringValue(PKEY_Identity_QualifiedUserName, &name)) &&
        SUCCEEDED(user->GetSid(&sid)) && name && sid) {
        auto* credential = new (std::nothrow) WristKeyProviderCredential(this, _scenario, name, sid);
        if (credential) {
            _credentials.push_back(credential);
            _haveCredentials = true;
        }
    }
    CoTaskMemFree(name);
    CoTaskMemFree(sid);
    user->Release();
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProvider::GetCredentialCount(DWORD* count, DWORD* def, BOOL* autoLogon) {
    if (!count || !def || !autoLogon) return E_POINTER;
    *count = 0;
    *def = CREDENTIAL_PROVIDER_NO_DEFAULT;
    *autoLogon = FALSE;

    if (_recreateCredentials) {
        _recreateCredentials = false;
        const HRESULT hr = CreateCredentials();
        if (FAILED(hr)) return hr;
    }
    *count = static_cast<DWORD>(_credentials.size());

    if (!_credentials.empty()) {
        const DWORD now = GetTickCount();
        const bool cooledDown = _lastAutoLogonTick == 0 || now - _lastAutoLogonTick > kAutoLogonCooldownMs;
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
    return _credentials[index]->QueryInterface(IID_ICredentialProviderCredential, reinterpret_cast<void**>(out));
}

void WristKeyProvider::RefreshStatus() {
    ICredentialProviderEvents* events = _events;  // stable: UnAdvise joins the watcher first
    if (events) events->CredentialsChanged(_adviseContext);
}

// -----------------------------------------------------------------------------
// WristKeyProviderCredential
// -----------------------------------------------------------------------------

WristKeyProviderCredential::WristKeyProviderCredential(WristKeyProvider* provider,
                                                       CREDENTIAL_PROVIDER_USAGE_SCENARIO scenario,
                                                       std::wstring username, std::wstring sid)
    : _provider(provider), _scenario(scenario), _username(std::move(username)), _sid(std::move(sid)) {
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

ULONG STDMETHODCALLTYPE WristKeyProviderCredential::AddRef() { return InterlockedIncrement(&_ref); }

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
    *autoLogon = FALSE;  // auto-logon is decided in GetCredentialCount
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::SetDeselected() { return S_OK; }

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetFieldState(
    DWORD id, CREDENTIAL_PROVIDER_FIELD_STATE* state, CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE* interactive) {
    if (!state || !interactive) return E_POINTER;
    switch (id) {
    case WristKeyFields::TileImage:
    case WristKeyFields::Tile:
        *state = CPFS_DISPLAY_IN_BOTH;
        *interactive = CPFIS_NONE;
        return S_OK;
    case WristKeyFields::Status:
        *state = CPFS_DISPLAY_IN_SELECTED_TILE;
        *interactive = CPFIS_NONE;
        return S_OK;
    case WristKeyFields::Submit:
        *state = CPFS_DISPLAY_IN_SELECTED_TILE;
        *interactive = CPFIS_FOCUSED;
        return S_OK;
    default:
        return E_INVALIDARG;
    }
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetStringValue(DWORD id, PWSTR* value) {
    if (!value) return E_POINTER;
    *value = nullptr;
    if (id == WristKeyFields::Tile) {
        const size_t slash = _username.find(L'\\');
        const std::wstring shortName = slash == std::wstring::npos ? _username : _username.substr(slash + 1);
        return CopyString((L"WristKey — " + shortName).c_str(), value);
    }
    if (id == WristKeyFields::Status) return CopyString(L"Confirm on your watch", value);
    return E_INVALIDARG;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetBitmapValue(DWORD id, HBITMAP* bitmap) {
    if (!bitmap) return E_POINTER;
    *bitmap = nullptr;
    if (id != WristKeyFields::TileImage) return E_INVALIDARG;

    // Draw a simple 128x128 key-coloured tile (no resources to ship).
    HDC screen = GetDC(nullptr);
    HDC mem = CreateCompatibleDC(screen);
    HBITMAP bmp = CreateCompatibleBitmap(screen, 128, 128);
    if (!mem || !bmp) {
        if (bmp) DeleteObject(bmp);
        if (mem) DeleteDC(mem);
        ReleaseDC(nullptr, screen);
        return E_FAIL;
    }
    HGDIOBJ old = SelectObject(mem, bmp);
    HBRUSH bg = CreateSolidBrush(RGB(0x1E, 0x6F, 0xEB));
    RECT rc{0, 0, 128, 128};
    FillRect(mem, &rc, bg);
    DeleteObject(bg);
    SetBkMode(mem, TRANSPARENT);
    SetTextColor(mem, RGB(255, 255, 255));
    HFONT font = CreateFontW(72, 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                             CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, DEFAULT_PITCH, L"Segoe UI");
    HGDIOBJ oldFont = SelectObject(mem, font);
    DrawTextW(mem, L"WK", -1, &rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    SelectObject(mem, oldFont);
    DeleteObject(font);
    SelectObject(mem, old);
    DeleteDC(mem);
    ReleaseDC(nullptr, screen);
    *bitmap = bmp;
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

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::SetStringValue(DWORD, PCWSTR) { return E_NOTIMPL; }
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::SetCheckboxValue(DWORD, BOOL) { return E_NOTIMPL; }
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::SetComboBoxSelectedValue(DWORD, DWORD) { return E_NOTIMPL; }
HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::CommandLinkClicked(DWORD) { return E_NOTIMPL; }

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetSerialization(
    CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE* response, CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* serialization,
    PWSTR* status, CREDENTIAL_PROVIDER_STATUS_ICON* icon) {
    if (!response || !serialization || !status || !icon) return E_POINTER;

    ZeroMemory(serialization, sizeof(*serialization));
    *response = CPGSR_NO_CREDENTIAL_NOT_FINISHED;
    *status = nullptr;
    *icon = CPSI_NONE;

    std::wstring password, error;
    if (!RequestUnlockFromDaemon(password, error, _events, static_cast<ICredentialProviderCredential*>(this))) {
        CopyString(error.empty() ? L"WristKey: confirm the unlock on your watch." : error.c_str(), status);
        *icon = CPSI_WARNING;
        return S_OK;
    }

    const HRESULT hr = PackUnlockCredential(_username, password, _scenario, serialization);
    if (!password.empty()) SecureZeroMemory(&password[0], password.size() * sizeof(wchar_t));

    if (SUCCEEDED(hr)) {
        *response = CPGSR_RETURN_CREDENTIAL_FINISHED;
        *icon = CPSI_SUCCESS;
    } else {
        CopyString((L"WristKey: could not build the credential. " + WinErrorText(static_cast<DWORD>(hr & 0xFFFF))).c_str(), status);
        *icon = CPSI_ERROR;
    }
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::ReportResult(
    NTSTATUS ntsStatus, NTSTATUS ntsSubstatus, PWSTR* status, CREDENTIAL_PROVIDER_STATUS_ICON* icon) {
    if (status) *status = nullptr;
    if (icon) *icon = CPSI_NONE;
    if (ntsStatus == 0 && ntsSubstatus == 0) return S_OK;

    const NTSTATUS detail = ntsSubstatus != 0 ? ntsSubstatus : ntsStatus;
    if (status) CopyString((L"WristKey: " + WinErrorText(LsaNtStatusToWinError(detail))).c_str(), status);
    if (icon) *icon = CPSI_ERROR;
    return S_OK;
}

HRESULT STDMETHODCALLTYPE WristKeyProviderCredential::GetUserSid(PWSTR* sid) {
    if (!sid) return E_POINTER;
    *sid = nullptr;
    if (_sid.empty()) return E_FAIL;
    return CopyString(_sid.c_str(), sid);
}
