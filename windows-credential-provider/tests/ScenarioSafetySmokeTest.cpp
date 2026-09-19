#include <windows.h>
#include <credentialprovider.h>
#include <iostream>

using DllGetClassObjectFn = HRESULT (STDAPICALLTYPE*)(REFCLSID, REFIID, void**);

static const GUID CLSID_WristKeyCredentialProvider =
    {0x7E1B7B8A, 0x4C8B, 0x4C2F, {0x9D, 0x8A, 0x8D, 0x3A, 0x7F, 0x2E, 0x51, 0xA1}};

static int Check(HRESULT actual, HRESULT expected, const wchar_t* label) {
    if (actual != expected) {
        std::wcerr << label << L": expected 0x" << std::hex
                   << static_cast<unsigned long>(expected)
                   << L", got 0x" << static_cast<unsigned long>(actual) << L"\n";
        return 1;
    }
    return 0;
}

int wmain(int argc, wchar_t** argv) {
    if (argc != 2) {
        std::wcerr << L"Usage: ScenarioSafetySmokeTest.exe <path-to-WristKeyCredentialProvider.dll>\n";
        return 2;
    }

    HMODULE module = LoadLibraryW(argv[1]);
    if (!module) {
        std::wcerr << L"LoadLibraryW failed: " << GetLastError() << L"\n";
        return 3;
    }

    auto cleanup = [&]() { FreeLibrary(module); };

    auto getClassObject = reinterpret_cast<DllGetClassObjectFn>(
        GetProcAddress(module, "DllGetClassObject"));
    if (!getClassObject) {
        cleanup();
        return 4;
    }

    IClassFactory* factory = nullptr;
    HRESULT hr = getClassObject(
        CLSID_WristKeyCredentialProvider,
        IID_IClassFactory,
        reinterpret_cast<void**>(&factory));
    if (FAILED(hr) || !factory) {
        cleanup();
        return 5;
    }

    ICredentialProvider* provider = nullptr;
    hr = factory->CreateInstance(
        nullptr,
        IID_ICredentialProvider,
        reinterpret_cast<void**>(&provider));
    factory->Release();
    if (FAILED(hr) || !provider) {
        cleanup();
        return 6;
    }

    // Normal sign-in: WristKey must stay out of CPUS_LOGON so the built-in
    // password/PIN providers remain the only normal sign-in credentials.
    if (Check(provider->SetUsageScenario(CPUS_LOGON, 0), E_NOTIMPL,
              L"CPUS_LOGON")) {
        provider->Release();
        cleanup();
        return 7;
    }

    // Unlock: WristKey remains available.
    if (Check(provider->SetUsageScenario(CPUS_UNLOCK_WORKSTATION, 0), S_OK,
              L"CPUS_UNLOCK_WORKSTATION")) {
        provider->Release();
        cleanup();
        return 8;
    }

    // Generic Credential UI: participate safely but expose no tiles.
    if (Check(provider->SetUsageScenario(CPUS_CREDUI, 0), S_OK,
              L"CPUS_CREDUI")) {
        provider->Release();
        cleanup();
        return 9;
    }

    DWORD count = 99;
    DWORD defaultIndex = 99;
    BOOL autoLogon = TRUE;
    hr = provider->GetCredentialCount(&count, &defaultIndex, &autoLogon);
    if (FAILED(hr) || count != 0 || defaultIndex != CREDENTIAL_PROVIDER_NO_DEFAULT || autoLogon) {
        std::wcerr << L"CPUS_CREDUI must enumerate zero credentials.\n";
        provider->Release();
        cleanup();
        return 10;
    }

    provider->Release();
    cleanup();

    std::wcout << L"Scenario safety smoke test passed.\n";
    return 0;
}
