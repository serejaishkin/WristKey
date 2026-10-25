#pragma once

#include <windows.h>
#include <string>
#include <vector>

bool ExtractJsonStringValue(const std::string& text, const std::string& key, std::string& out);
std::wstring Utf8ToWide(const std::string& text);

static bool RequestUnlockFromDaemon(std::wstring& outPassword, std::wstring& outError,
                                     ICredentialProviderCredentialEvents* events = nullptr,
                                     ICredentialProviderCredential* credential = nullptr);

static bool QueryExpectedUsernameFromDaemon(std::wstring& outUsername);
static bool QueryWatchPresentFromDaemon();

static HRESULT PackUnlockCredential(
    const std::wstring& qualifiedUsername,
    const std::wstring& password,
    CREDENTIAL_PROVIDER_USAGE_SCENARIO scenario,
    CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* out);

static std::wstring WinErrorText(DWORD error);
