#requires -RunAsAdministrator
param(
    [ValidateSet('Install','Uninstall')]
    [string]$Action = 'Install',
    [string]$DllPath = "$PSScriptRoot\build\Release\WristKeyCredentialProvider.dll"
)

$ErrorActionPreference = 'Stop'
if (-not [Environment]::Is64BitOperatingSystem) { throw 'WristKey Credential Provider requires 64-bit Windows.' }
if (-not [Environment]::Is64BitProcess) { throw 'Run this script from 64-bit PowerShell.' }

$target = Join-Path $env:WINDIR 'System32\WristKeyCredentialProvider.dll'
$regsvr32 = Join-Path $env:WINDIR 'System32\regsvr32.exe'
$nativeClsid = '{7E1B7B8A-4C8B-4C2F-9D8A-8D3A7F2E51A1}'
$legacyManagedClsid = '{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}'

function Remove-LegacyManagedProvider {
    $paths = @(
        "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\$legacyManagedClsid",
        "HKLM:\SOFTWARE\Classes\CLSID\$legacyManagedClsid"
    )
    foreach ($path in $paths) {
        if (Test-Path $path) {
            Remove-Item -LiteralPath $path -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

if ($Action -eq 'Install') {
    if (-not (Test-Path -LiteralPath $DllPath)) { throw "Native Credential Provider DLL not found: $DllPath" }

    Remove-LegacyManagedProvider
    Copy-Item -LiteralPath $DllPath -Destination $target -Force

    # Native provider exports DllRegisterServer/DllUnregisterServer and registers
    # both COM and the Windows Credential Provider mapping.
    & $regsvr32 /s $target
    if ($LASTEXITCODE -ne 0) {
        Remove-Item -LiteralPath $target -Force -ErrorAction SilentlyContinue
        throw "regsvr32 failed: $LASTEXITCODE"
    }

    $cpKey = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\$nativeClsid"
    $inprocKey = "HKLM:\SOFTWARE\Classes\CLSID\$nativeClsid\InprocServer32"
    if (-not (Test-Path $cpKey) -or -not (Test-Path $inprocKey)) {
        throw 'Credential Provider registration is incomplete.'
    }

    Write-Host "WristKey native V2 Credential Provider installed: $target"
    Write-Host 'Reboot Windows (or sign out/in) and open Sign-in options.'
    Write-Host 'The WristKey icon should appear there alongside PIN/password.'
} else {
    if (Test-Path -LiteralPath $target) {
        & $regsvr32 /u /s $target
        Remove-Item -LiteralPath $target -Force -ErrorAction SilentlyContinue
    }
    Remove-LegacyManagedProvider
    Write-Host 'WristKey Credential Provider uninstalled.'
}
