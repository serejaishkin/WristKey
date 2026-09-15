#include <windows.h>
#include <iostream>

using DllRegisterServerFn = HRESULT (STDAPICALLTYPE*)();
using DllUnregisterServerFn = HRESULT (STDAPICALLTYPE*)();

static void PrintHr(const wchar_t* label, HRESULT hr) {
    std::wcerr << label << L" 0x" << std::hex << static_cast<unsigned long>(hr)
               << L" (" << std::dec << static_cast<long>(hr) << L")\n";
}

int wmain(int argc, wchar_t** argv) {
    if (argc != 2) {
        std::wcerr << L"Usage: RegistrationSmokeTest.exe <path-to-WristKeyCredentialProvider.dll>\n";
        return 2;
    }

    HMODULE module = LoadLibraryW(argv[1]);
    if (!module) {
        std::wcerr << L"LoadLibraryW failed: " << GetLastError() << L"\n";
        return 3;
    }

    auto cleanup = [&]() { FreeLibrary(module); };

    auto reg = reinterpret_cast<DllRegisterServerFn>(GetProcAddress(module, "DllRegisterServer"));
    auto unreg = reinterpret_cast<DllUnregisterServerFn>(GetProcAddress(module, "DllUnregisterServer"));
    if (!reg || !unreg) {
        std::wcerr << L"Registration exports not found.\n";
        cleanup();
        return 4;
    }

    HRESULT hr = reg();
    PrintHr(L"DllRegisterServer:", hr);
    if (FAILED(hr)) {
        HRESULT cleanupHr = unreg();
        PrintHr(L"DllUnregisterServer (cleanup):", cleanupHr);
        cleanup();
        return 5;
    }

    hr = unreg();
    PrintHr(L"DllUnregisterServer:", hr);
    cleanup();

    return SUCCEEDED(hr) ? 0 : 6;
}
