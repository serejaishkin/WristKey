#include <windows.h>
#include <credentialprovider.h>
#include <iostream>

using DllGetClassObjectFn = HRESULT (STDAPICALLTYPE*)(REFCLSID, REFIID, void**);

static const GUID CLSID_WristKeyCredentialProvider =
    {0x7E1B7B8A, 0x4C8B, 0x4C2F, {0x9D, 0x8A, 0x8D, 0x3A, 0x7F, 0x2E, 0x51, 0xA1}};

int wmain(int argc, wchar_t** argv) {
    if (argc != 2) {
        std::wcerr << L"Usage: ProviderSmokeTest.exe <path-to-WristKeyCredentialProvider.dll>\n";
        return 2;
    }

    HMODULE module = LoadLibraryW(argv[1]);
    if (!module) {
        std::wcerr << L"LoadLibraryW failed: " << GetLastError() << L"\n";
        return 3;
    }

    auto getClassObject = reinterpret_cast<DllGetClassObjectFn>(
        GetProcAddress(module, "DllGetClassObject"));
    if (!getClassObject) {
        std::wcerr << L"DllGetClassObject export not found.\n";
        FreeLibrary(module);
        return 4;
    }

    IClassFactory* factory = nullptr;
    HRESULT hr = getClassObject(CLSID_WristKeyCredentialProvider,
                                IID_IClassFactory,
                                reinterpret_cast<void**>(&factory));
    if (FAILED(hr) || !factory) {
        std::wcerr << L"DllGetClassObject failed: 0x" << std::hex << hr << L"\n";
        FreeLibrary(module);
        return 5;
    }

    ICredentialProvider* provider = nullptr;
    hr = factory->CreateInstance(nullptr, IID_ICredentialProvider,
                                 reinterpret_cast<void**>(&provider));
    factory->Release();

    if (FAILED(hr) || !provider) {
        std::wcerr << L"CreateInstance failed: 0x" << std::hex << hr << L"\n";
        FreeLibrary(module);
        return 6;
    }

    hr = provider->SetUsageScenario(CPUS_LOGON, 0);
    if (FAILED(hr)) {
        std::wcerr << L"SetUsageScenario(CPUS_LOGON) failed: 0x" << std::hex << hr << L"\n";
        provider->Release();
        FreeLibrary(module);
        return 7;
    }

    hr = provider->SetUsageScenario(CPUS_UNLOCK_WORKSTATION, 0);
    if (FAILED(hr)) {
        std::wcerr << L"SetUsageScenario(CPUS_UNLOCK_WORKSTATION) failed: 0x" << std::hex << hr << L"\n";
        provider->Release();
        FreeLibrary(module);
        return 8;
    }

    ICredentialProviderSetUserArray* setUserArray = nullptr;
    hr = provider->QueryInterface(IID_ICredentialProviderSetUserArray,
                                  reinterpret_cast<void**>(&setUserArray));
    if (FAILED(hr) || !setUserArray) {
        std::wcerr << L"ICredentialProviderSetUserArray QI failed: 0x" << std::hex << hr << L"\n";
        provider->Release();
        FreeLibrary(module);
        return 9;
    }
    setUserArray->Release();

    DWORD count = 0;
    DWORD defaultIndex = CREDENTIAL_PROVIDER_NO_DEFAULT;
    BOOL autoLogon = FALSE;
    hr = provider->GetCredentialCount(&count, &defaultIndex, &autoLogon);
    if (FAILED(hr)) {
        std::wcerr << L"GetCredentialCount failed: 0x" << std::hex << hr << L"\n";
        provider->Release();
        FreeLibrary(module);
        return 10;
    }

    std::wcout << L"Provider COM smoke test passed.\n";
    std::wcout << L"Credential count without user array: " << count << L"\n";

    provider->Release();
    FreeLibrary(module);
    return 0;
}
