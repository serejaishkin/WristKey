#include <windows.h>
#include <credentialprovider.h>
#include <iostream>

using DllGetClassObjectFn = HRESULT (STDAPICALLTYPE*)(REFCLSID, REFIID, void**);

static const GUID CLSID_WristKeyCredentialProvider =
    {0x7E1B7B8A, 0x4C8B, 0x4C2F, {0x9D, 0x8A, 0x8D, 0x3A, 0x7F, 0x2E, 0x51, 0xA1}};

static int Fail(const wchar_t* message, HRESULT hr, int code) {
    std::wcerr << message << L" 0x" << std::hex << hr << L"\n";
    return code;
}

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

    auto cleanup = [&]() { FreeLibrary(module); };

    auto getClassObject = reinterpret_cast<DllGetClassObjectFn>(
        GetProcAddress(module, "DllGetClassObject"));
    if (!getClassObject) {
        std::wcerr << L"DllGetClassObject export not found.\n";
        cleanup();
        return 4;
    }

    IClassFactory* factory = nullptr;
    HRESULT hr = getClassObject(CLSID_WristKeyCredentialProvider,
                                IID_IClassFactory,
                                reinterpret_cast<void**>(&factory));
    if (FAILED(hr) || !factory) {
        int code = Fail(L"DllGetClassObject failed:", hr, 5);
        cleanup();
        return code;
    }

    ICredentialProvider* provider = nullptr;
    hr = factory->CreateInstance(nullptr, IID_ICredentialProvider,
                                 reinterpret_cast<void**>(&provider));
    factory->Release();

    if (FAILED(hr) || !provider) {
        int code = Fail(L"CreateInstance failed:", hr, 6);
        cleanup();
        return code;
    }

    hr = provider->SetUsageScenario(CPUS_LOGON, 0);
    if (FAILED(hr)) {
        int code = Fail(L"SetUsageScenario(CPUS_LOGON) failed:", hr, 7);
        provider->Release();
        cleanup();
        return code;
    }

    hr = provider->SetUsageScenario(CPUS_UNLOCK_WORKSTATION, 0);
    if (FAILED(hr)) {
        int code = Fail(L"SetUsageScenario(CPUS_UNLOCK_WORKSTATION) failed:", hr, 8);
        provider->Release();
        cleanup();
        return code;
    }

    ICredentialProviderSetUserArray* setUserArray = nullptr;
    hr = provider->QueryInterface(IID_ICredentialProviderSetUserArray,
                                  reinterpret_cast<void**>(&setUserArray));
    if (FAILED(hr) || !setUserArray) {
        int code = Fail(L"ICredentialProviderSetUserArray QI failed:", hr, 9);
        provider->Release();
        cleanup();
        return code;
    }
    setUserArray->Release();

    DWORD fieldCount = 0;
    hr = provider->GetFieldDescriptorCount(&fieldCount);
    if (FAILED(hr)) {
        int code = Fail(L"GetFieldDescriptorCount failed:", hr, 10);
        provider->Release();
        cleanup();
        return code;
    }
    if (fieldCount != 4) {
        std::wcerr << L"Unexpected field count: " << fieldCount << L" (expected 4)\n";
        provider->Release();
        cleanup();
        return 11;
    }

    for (DWORD i = 0; i < fieldCount; ++i) {
        CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR* descriptor = nullptr;
        hr = provider->GetFieldDescriptorAt(i, &descriptor);
        if (FAILED(hr) || !descriptor) {
            int code = Fail(L"GetFieldDescriptorAt failed:", hr, 12);
            provider->Release();
            cleanup();
            return code;
        }

        if (descriptor->dwFieldID != i || !descriptor->pszLabel) {
            std::wcerr << L"Invalid descriptor returned for field " << i << L"\n";
            CoTaskMemFree(descriptor->pszLabel);
            CoTaskMemFree(descriptor);
            provider->Release();
            cleanup();
            return 13;
        }

        CoTaskMemFree(descriptor->pszLabel);
        CoTaskMemFree(descriptor);
    }

    DWORD count = 0;
    DWORD defaultIndex = CREDENTIAL_PROVIDER_NO_DEFAULT;
    BOOL autoLogon = FALSE;
    hr = provider->GetCredentialCount(&count, &defaultIndex, &autoLogon);
    if (FAILED(hr)) {
        int code = Fail(L"GetCredentialCount failed:", hr, 14);
        provider->Release();
        cleanup();
        return code;
    }

    std::wcout << L"Provider COM smoke test passed.\n";
    std::wcout << L"Field descriptors: " << fieldCount << L"\n";
    std::wcout << L"Credential count without user array: " << count << L"\n";

    provider->Release();
    cleanup();
    return 0;
}
