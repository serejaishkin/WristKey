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

function Invoke-Regsvr32 {
    param([string[]]$Arguments)
    # On some Windows builds (e.g. 10.0.28000) regsvr32 does not populate
    # $LASTEXITCODE, so a naive `& regsvr32; if ($LASTEXITCODE -ne 0)` check
    # reports a false failure ($null -ne 0). Start-Process -PassThru reports
    # the real process exit code reliably.
    $proc = Start-Process -FilePath $regsvr32 -ArgumentList $Arguments -Wait -PassThru
    if ($proc.ExitCode -ne 0) { throw "regsvr32 exited with code $($proc.ExitCode)" }
}

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
    try {
        Invoke-Regsvr32 @('/s', $target)
    } catch {
        Remove-Item -LiteralPath $target -Force -ErrorAction SilentlyContinue
        throw
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
        Invoke-Regsvr32 @('/u', '/s', $target)
        Remove-Item -LiteralPath $target -Force -ErrorAction SilentlyContinue
    }
    Remove-LegacyManagedProvider
    Write-Host 'WristKey Credential Provider uninstalled.'
}
