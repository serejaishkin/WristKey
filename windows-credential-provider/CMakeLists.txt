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
)

target_compile_features(WristKeyCredentialProvider PRIVATE cxx_std_17)
target_compile_definitions(WristKeyCredentialProvider PRIVATE
    UNICODE _UNICODE
    WIN32_LEAN_AND_MEAN
    NOMINMAX
    _WIN32_WINNT=0x0A00
)

target_include_directories(WristKeyCredentialProvider PRIVATE include)
target_link_libraries(WristKeyCredentialProvider PRIVATE ole32 oleaut32 advapi32 user32 netapi32)

set_target_properties(WristKeyCredentialProvider PROPERTIES
    OUTPUT_NAME "WristKeyCredentialProvider"
    WINDOWS_EXPORT_ALL_SYMBOLS OFF
)

if(CMAKE_SIZEOF_VOID_P EQUAL 8)
    message(STATUS "Building x64 Credential Provider")
else()
    message(FATAL_ERROR "WristKey Credential Provider must be built x64")
endif()
