#Requires -RunAsAdministrator
<#
.SYNOPSIS
    Registers WristKey Credential Provider for Windows logon/unlock screen.
.DESCRIPTION
    Creates registry entries for COM and Credential Provider.
    Run this script after building WristKeyCredentialProvider.dll.
#>

param(
    [string]$DllPath = "C:\Program Files\WristKey\WristKeyCredentialProvider.dll"
)

$clsid = "{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}"
$name = "WristKey Credential Provider"

# Ensure directory exists
$dir = Split-Path $DllPath -Parent
if (-not (Test-Path $dir)) {
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
}

if (-not (Test-Path $DllPath)) {
    Write-Error "DLL not found at $DllPath. Build the project first."
    Write-Host ""
    Write-Host "Build instructions:"
    Write-Host "  1. Open WristKeyCredentialProvider.csproj in Visual Studio or use MSBuild"
    Write-Host "  2. Build in Release mode (x64)"
    Write-Host "  3. Copy output DLL to $DllPath"
    exit 1
}

# This is a managed .NET Framework assembly. Credential providers are loaded by
# COM, so direct InprocServer32=<path-to-dll> registration is invalid; RegAsm
# must create the mscoree/Assembly/Class registration entries.
$regAsm = Join-Path $env:WINDIR "Microsoft.NET\Framework64\v4.0.30319\RegAsm.exe"
if (-not (Test-Path $regAsm)) {
    Write-Error "64-bit .NET Framework RegAsm was not found at $regAsm"
    exit 1
}
& $regAsm $DllPath /codebase
if ($LASTEXITCODE -ne 0) {
    Write-Error "RegAsm failed with exit code $LASTEXITCODE"
    exit $LASTEXITCODE
}

# Register as Credential Provider
$cpPath = "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\$clsid"
New-Item -Path $cpPath -Force | Out-Null
Set-ItemProperty -Path $cpPath -Name "(Default)" -Value $name

Write-Host "WristKey Credential Provider registered successfully!" -ForegroundColor Green
Write-Host "CLSID: $clsid" -ForegroundColor Cyan
Write-Host "DLL: $DllPath" -ForegroundColor Cyan
Write-Host ""
Write-Host "IMPORTANT: Ensure WristKey daemon is running before lock screen." -ForegroundColor Yellow
Write-Host "  Start daemon: wristkeyd.exe or via Tauri app" -ForegroundColor Yellow
Write-Host ""
Write-Host "Restart your computer or run 'shutdown /r /t 0' to apply changes." -ForegroundColor Yellow
