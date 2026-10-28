#pragma once

#include <windows.h>
#include <string>
#include <vector>

// Forward declarations for Windows API functions not in MinGW headers
extern "C" {
    NTSTATUS WINAPI LsaNtStatusToWinError(NTSTATUS Status);
}

// Forward declarations for Credential Provider fields
