#requires -RunAsAdministrator
param(
    [ValidateSet('Install','Uninstall')]
    [string]$Action = 'Install',
    [string]$DllPath = "$PSScriptRoot\build\WristKeyCredentialProvider.dll"
)

$ErrorActionPreference = 'Stop'
if (-not [Environment]::Is64BitOperatingSystem) { throw 'WristKey Credential Provider requires 64-bit Windows.' }
if (-not [Environment]::Is64BitProcess) { throw 'Run this script from 64-bit PowerShell.' }

$target = Join-Path $env:WINDIR 'System32\WristKeyCredentialProvider.dll'
$regsvr32 = Join-Path $env:WINDIR 'System32\regsvr32.exe'

if ($Action -eq 'Install') {
    if (-not (Test-Path -LiteralPath $DllPath)) { throw "DLL not found: $DllPath" }
    Copy-Item -LiteralPath $DllPath -Destination $target -Force
    & $regsvr32 /s $target
    if ($LASTEXITCODE -ne 0) { Remove-Item -LiteralPath $target -Force -ErrorAction SilentlyContinue; throw "regsvr32 failed: $LASTEXITCODE" }
    Write-Host "WristKey Credential Provider installed: $target"
    Write-Host 'The provider is intentionally a UI/IPC skeleton until daemon serialization is wired.'
} else {
    if (Test-Path -LiteralPath $target) {
        & $regsvr32 /u /s $target
        Remove-Item -LiteralPath $target -Force -ErrorAction SilentlyContinue
    }
    Write-Host 'WristKey Credential Provider uninstalled.'
}
