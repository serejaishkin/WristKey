#include "helpers.h"

#include <strsafe.h>
#include <new>
#include <string>

#pragma comment(lib, "advapi32.lib")
#pragma comment(lib, "secur32.lib")
#pragma comment(lib, "shlwapi.lib")

// Adapted from Microsoft's Windows-classic-samples Credential Provider V2 sample.
// Copyright (c) Microsoft Corporation. All rights reserved.

static HRESULT CopyTaskString(PCWSTR src, PWSTR* dst)
{
    if (!dst) return E_POINTER;
    *dst = nullptr;
    if (!src) return E_INVALIDARG;

    const size_t chars = wcslen(src) + 1;
    if (chars > (SIZE_MAX / sizeof(wchar_t))) return E_OUTOFMEMORY;

    auto* p = static_cast<PWSTR>(CoTaskMemAlloc(chars * sizeof(wchar_t)));
    if (!p) return E_OUTOFMEMORY;
    memcpy(p, src, chars * sizeof(wchar_t));
    *dst = p;
    return S_OK;
}

static HRESULT UnicodeStringInitWithString(PWSTR pwz, UNICODE_STRING* pus)
{
    if (!pwz || !pus) return E_INVALIDARG;

    const size_t chars = wcslen(pwz);
    if (chars > USHRT_MAX / sizeof(wchar_t)) return HRESULT_FROM_WIN32(ERROR_ARITHMETIC_OVERFLOW);

    pus->Length = static_cast<USHORT>(chars * sizeof(wchar_t));
    pus->MaximumLength = pus->Length;
    pus->Buffer = pwz;
    return S_OK;
}

HRESULT KerbInteractiveUnlockLogonInit(
    PWSTR pwzDomain,
    PWSTR pwzUsername,
    PWSTR pwzPassword,
    CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus,
    KERB_INTERACTIVE_UNLOCK_LOGON* pkiul)
{
    if (!pkiul) return E_POINTER;

    KERB_INTERACTIVE_UNLOCK_LOGON kiul{};
    auto* pkil = &kiul.Logon;

    HRESULT hr = UnicodeStringInitWithString(pwzDomain, &pkil->LogonDomainName);
    if (SUCCEEDED(hr)) hr = UnicodeStringInitWithString(pwzUsername, &pkil->UserName);
    if (SUCCEEDED(hr)) hr = UnicodeStringInitWithString(pwzPassword, &pkil->Password);

    if (SUCCEEDED(hr))
    {
        switch (cpus)
        {
        case CPUS_UNLOCK_WORKSTATION:
            pkil->MessageType = KerbWorkstationUnlockLogon;
            break;
        case CPUS_LOGON:
            pkil->MessageType = KerbInteractiveLogon;
            break;
        case CPUS_CREDUI:
            pkil->MessageType = static_cast<KERB_LOGON_SUBMIT_TYPE>(0);
            break;
        default:
            return E_INVALIDARG;
        }

        CopyMemory(pkiul, &kiul, sizeof(*pkiul));
    }

    return hr;
}

static void PackedUnicodeStringCopy(
    const UNICODE_STRING& source,
    PWSTR destination,
    UNICODE_STRING* target)
{
    target->Length = source.Length;
    target->MaximumLength = source.Length;
    target->Buffer = destination;
    if (source.Length && source.Buffer)
    {
        CopyMemory(destination, source.Buffer, source.Length);
    }
}

HRESULT KerbInteractiveUnlockLogonPack(
    const KERB_INTERACTIVE_UNLOCK_LOGON& rkiulIn,
    BYTE** prgb,
    DWORD* pcb)
{
    if (!prgb || !pcb) return E_POINTER;
    *prgb = nullptr;
    *pcb = 0;

    const auto* pkilIn = &rkiulIn.Logon;
    const size_t cb = sizeof(rkiulIn) +
                      pkilIn->LogonDomainName.Length +
                      pkilIn->UserName.Length +
                      pkilIn->Password.Length;
    if (cb > MAXDWORD) return HRESULT_FROM_WIN32(ERROR_ARITHMETIC_OVERFLOW);

    auto* pkiulOut = static_cast<KERB_INTERACTIVE_UNLOCK_LOGON*>(CoTaskMemAlloc(cb));
    if (!pkiulOut) return E_OUTOFMEMORY;
    ZeroMemory(pkiulOut, cb);

    BYTE* buffer = reinterpret_cast<BYTE*>(pkiulOut) + sizeof(*pkiulOut);
    auto* pkilOut = &pkiulOut->Logon;
    pkilOut->MessageType = pkilIn->MessageType;

    PackedUnicodeStringCopy(pkilIn->LogonDomainName,
                            reinterpret_cast<PWSTR>(buffer),
                            &pkilOut->LogonDomainName);
    pkilOut->LogonDomainName.Buffer = reinterpret_cast<PWSTR>(
        buffer - reinterpret_cast<BYTE*>(pkiulOut));
    buffer += pkilOut->LogonDomainName.Length;

    PackedUnicodeStringCopy(pkilIn->UserName,
                            reinterpret_cast<PWSTR>(buffer),
                            &pkilOut->UserName);
    pkilOut->UserName.Buffer = reinterpret_cast<PWSTR>(
        buffer - reinterpret_cast<BYTE*>(pkiulOut));
    buffer += pkilOut->UserName.Length;

    PackedUnicodeStringCopy(pkilIn->Password,
                            reinterpret_cast<PWSTR>(buffer),
                            &pkilOut->Password);
    pkilOut->Password.Buffer = reinterpret_cast<PWSTR>(
        buffer - reinterpret_cast<BYTE*>(pkiulOut));

    *prgb = reinterpret_cast<BYTE*>(pkiulOut);
    *pcb = static_cast<DWORD>(cb);
    return S_OK;
}

static HRESULT LsaInitString(PLSA_STRING destination, PCSTR source)
{
    if (!destination || !source) return E_INVALIDARG;
    const size_t length = strlen(source);
    if (length > USHRT_MAX - 1) return HRESULT_FROM_WIN32(ERROR_ARITHMETIC_OVERFLOW);

    destination->Buffer = const_cast<PCHAR>(source);
    destination->Length = static_cast<USHORT>(length);
    destination->MaximumLength = static_cast<USHORT>(length + 1);
    return S_OK;
}

HRESULT RetrieveNegotiateAuthPackage(ULONG* pulAuthPackage)
{
    if (!pulAuthPackage) return E_POINTER;
    *pulAuthPackage = 0;

    HANDLE hLsa = nullptr;
    NTSTATUS status = LsaConnectUntrusted(&hLsa);
    if (status != 0) return HRESULT_FROM_NT(status);

    LSA_STRING negotiate{};
    HRESULT hr = LsaInitString(&negotiate, NEGOSSP_NAME_A);
    ULONG authPackage = 0;
    if (SUCCEEDED(hr))
    {
        status = LsaLookupAuthenticationPackage(hLsa, &negotiate, &authPackage);
        hr = (status == 0) ? S_OK : HRESULT_FROM_NT(status);
    }

    LsaDeregisterLogonProcess(hLsa);
    if (SUCCEEDED(hr)) *pulAuthPackage = authPackage;
    return hr;
}

static HRESULT ProtectAndCopyString(PCWSTR source, PWSTR* destination)
{
    if (!source || !*source || !destination) return E_INVALIDARG;
    *destination = nullptr;

    PWSTR mutableSource = nullptr;
    HRESULT hr = CopyTaskString(source, &mutableSource);
    if (FAILED(hr)) return hr;

    DWORD protectedChars = 0;
    if (CredProtectW(FALSE, mutableSource,
                     static_cast<DWORD>(wcslen(mutableSource) + 1),
                     nullptr, &protectedChars, nullptr))
    {
        CoTaskMemFree(mutableSource);
        return E_UNEXPECTED;
    }

    DWORD error = GetLastError();
    if (error != ERROR_INSUFFICIENT_BUFFER || protectedChars == 0)
    {
        CoTaskMemFree(mutableSource);
        return HRESULT_FROM_WIN32(error);
    }

    auto* protectedPassword = static_cast<PWSTR>(
        CoTaskMemAlloc(protectedChars * sizeof(wchar_t)));
    if (!protectedPassword)
    {
        CoTaskMemFree(mutableSource);
        return E_OUTOFMEMORY;
    }

    if (!CredProtectW(FALSE, mutableSource,
                      static_cast<DWORD>(wcslen(mutableSource) + 1),
                      protectedPassword, &protectedChars, nullptr))
    {
        error = GetLastError();
        CoTaskMemFree(protectedPassword);
        CoTaskMemFree(mutableSource);
        return HRESULT_FROM_WIN32(error);
    }

    CoTaskMemFree(mutableSource);
    *destination = protectedPassword;
    return S_OK;
}

HRESULT ProtectIfNecessaryAndCopyPassword(
    PCWSTR pwzPassword,
    CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus,
    PWSTR* ppwzProtectedPassword)
{
    if (!ppwzProtectedPassword) return E_POINTER;
    *ppwzProtectedPassword = nullptr;

    if (!pwzPassword || !*pwzPassword)
    {
        return CopyTaskString(L"", ppwzProtectedPassword);
    }

    PWSTR passwordCopy = nullptr;
    HRESULT hr = CopyTaskString(pwzPassword, &passwordCopy);
    if (FAILED(hr)) return hr;

    CRED_PROTECTION_TYPE protectionType = CredUnprotected;
    const bool alreadyProtected = CredIsProtectedW(passwordCopy, &protectionType) &&
                                   protectionType != CredUnprotected;

    if (cpus == CPUS_CREDUI || alreadyProtected)
    {
        hr = CopyTaskString(passwordCopy, ppwzProtectedPassword);
    }
    else
    {
        hr = ProtectAndCopyString(passwordCopy, ppwzProtectedPassword);
    }

    if (!alreadyProtected && hr == S_OK)
    {
        SecureZeroMemory(passwordCopy, wcslen(passwordCopy) * sizeof(wchar_t));
    }
    CoTaskMemFree(passwordCopy);
    return hr;
}

HRESULT SplitDomainAndUsername(
    PCWSTR qualified,
    PWSTR* domain,
    PWSTR* username)
{
    if (!domain || !username) return E_POINTER;
    *domain = nullptr;
    *username = nullptr;
    if (!qualified || !*qualified) return E_INVALIDARG;

    const wchar_t* slash = wcschr(qualified, L'\\');
    if (!slash)
    {
        // A bare username is valid for a local account. Use the conventional
        // local-domain prefix so the Kerberos package receives an explicit identity.
        HRESULT hr = CopyTaskString(L".", domain);
        if (SUCCEEDED(hr)) hr = CopyTaskString(qualified, username);
        if (FAILED(hr))
        {
            CoTaskMemFree(*domain);
            CoTaskMemFree(*username);
            *domain = nullptr;
            *username = nullptr;
        }
        return hr;
    }

    const size_t domainLen = static_cast<size_t>(slash - qualified);
    const size_t userLen = wcslen(slash + 1);
    if (domainLen == 0 || userLen == 0) return E_INVALIDARG;
    if (domainLen > (SIZE_MAX / sizeof(wchar_t)) - 1 ||
        userLen > (SIZE_MAX / sizeof(wchar_t)) - 1)
    {
        return E_OUTOFMEMORY;
    }

    *domain = static_cast<PWSTR>(CoTaskMemAlloc((domainLen + 1) * sizeof(wchar_t)));
    if (!*domain) return E_OUTOFMEMORY;
    *username = static_cast<PWSTR>(CoTaskMemAlloc((userLen + 1) * sizeof(wchar_t)));
    if (!*username)
    {
        CoTaskMemFree(*domain);
        *domain = nullptr;
        return E_OUTOFMEMORY;
    }

    memcpy(*domain, qualified, domainLen * sizeof(wchar_t));
    (*domain)[domainLen] = L'\0';
    memcpy(*username, slash + 1, userLen * sizeof(wchar_t));
    (*username)[userLen] = L'\0';
    return S_OK;
}
