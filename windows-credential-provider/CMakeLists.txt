cmake_minimum_required(VERSION 3.20)
project(WristKeyCredentialProvider LANGUAGES CXX)

if(NOT WIN32)
    message(FATAL_ERROR "WristKey Credential Provider is Windows-only")
endif()

add_library(WristKeyCredentialProvider SHARED
    src/Guid.cpp
    src/Provider.cpp
    src/Credential.cpp
    src/exports.cpp
    src/helpers.cpp
)

target_compile_features(WristKeyCredentialProvider PRIVATE cxx_std_17)
target_compile_definitions(WristKeyCredentialProvider PRIVATE
    UNICODE _UNICODE
    WIN32_LEAN_AND_MEAN
    NOMINMAX
    _WIN32_WINNT=0x0A00
    SECURITY_WIN32
)

# Credential providers are loaded by LogonUI, before a normal user session
# exists. Keep the DLL self-contained and avoid a dependency on the VC++ CRT
# redistributable that may not be present at the logon screen.
if(MSVC)
    set_property(TARGET WristKeyCredentialProvider PROPERTY
        MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")
endif()

target_include_directories(WristKeyCredentialProvider PRIVATE include)
target_link_libraries(WristKeyCredentialProvider PRIVATE
    ole32
    oleaut32
    advapi32
    user32
    netapi32
    credui
    secur32
    shlwapi
)

set_target_properties(WristKeyCredentialProvider PROPERTIES
    OUTPUT_NAME "WristKeyCredentialProvider"
    WINDOWS_EXPORT_ALL_SYMBOLS OFF
    LINK_FLAGS "/DEF:${CMAKE_CURRENT_SOURCE_DIR}/WristKeyCredentialProvider.def"
)

if(CMAKE_SIZEOF_VOID_P EQUAL 8)
    message(STATUS "Building x64 Credential Provider")
else()
    message(FATAL_ERROR "WristKey Credential Provider must be built x64")
endif()

# Safe local COM smoke test: loads the DLL directly and exercises the COM
# factory/provider interfaces without registering the provider in LogonUI.
add_executable(ProviderSmokeTest tests/ProviderSmokeTest.cpp)
target_compile_features(ProviderSmokeTest PRIVATE cxx_std_17)
target_compile_definitions(ProviderSmokeTest PRIVATE
    UNICODE _UNICODE
    WIN32_LEAN_AND_MEAN
    NOMINMAX
    _WIN32_WINNT=0x0A00
)
target_link_libraries(ProviderSmokeTest PRIVATE ole32 oleaut32 user32)

# Safe registration smoke test: calls DllRegisterServer/DllUnregisterServer
# directly and reports the exact HRESULT. It is not installed into LogonUI.
add_executable(RegistrationSmokeTest tests/RegistrationSmokeTest.cpp)
target_compile_features(RegistrationSmokeTest PRIVATE cxx_std_17)
target_compile_definitions(RegistrationSmokeTest PRIVATE
    UNICODE _UNICODE
    WIN32_LEAN_AND_MEAN
    NOMINMAX
    _WIN32_WINNT=0x0A00
)
target_link_libraries(RegistrationSmokeTest PRIVATE ole32 oleaut32 advapi32)
