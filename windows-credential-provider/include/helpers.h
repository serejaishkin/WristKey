#pragma once

// Adapted from Microsoft's Windows-classic-samples Credential Provider V2 sample.
// Copyright (c) Microsoft Corporation. All rights reserved.

#include <windows.h>
#include <credentialprovider.h>
#include <ntsecapi.h>
#include <wincred.h>
#include <security.h>

HRESULT KerbInteractiveUnlockLogonInit(
    PWSTR pwzDomain,
    PWSTR pwzUsername,
    PWSTR pwzPassword,
    CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus,
    KERB_INTERACTIVE_UNLOCK_LOGON* pkiul);

HRESULT KerbInteractiveUnlockLogonPack(
    const KERB_INTERACTIVE_UNLOCK_LOGON& rkiulIn,
    BYTE** prgb,
    DWORD* pcb);

HRESULT RetrieveNegotiateAuthPackage(ULONG* pulAuthPackage);

HRESULT ProtectIfNecessaryAndCopyPassword(
    PCWSTR pwzPassword,
    CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus,
    PWSTR* ppwzProtectedPassword);

HRESULT SplitDomainAndUsername(
    PCWSTR pszQualifiedUserName,
    PWSTR* ppszDomain,
    PWSTR* ppszUsername);
