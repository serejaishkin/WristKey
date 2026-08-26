#Requires -RunAsAdministrator
<#
.SYNOPSIS
    Registers WristKey Credential Provider for Windows logon/unlock screen.
.DESCRIPTION
    Creates registry entries for COM and Credential Provider.
    Run this script after building WristKeyCredentialProvider.dll.
#>

param(
    [string]$DllPath = "C:\Program Files\WristKey\WristKeyCredentialProvider.dll",
    # Build output directory containing WristKeyCredentialProvider.dll and
    # its dependency DLLs (e.g. Newtonsoft.Json.dll). All *.dll from this
    # directory are copied next to the target so LogonUI can load them.
    [string]$SourceDir = (Join-Path $PSScriptRoot "bin\Release\net48")
)

$clsid = "{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}"
$name = "WristKey Credential Provider"

# Ensure directory exists
$dir = Split-Path $DllPath -Parent
if (-not (Test-Path $dir)) {
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
}

# Always refresh the installed DLLs from the build output so a re-run
# updates an existing installation and its dependencies (Newtonsoft.Json).
$builtDll = Join-Path $SourceDir "WristKeyCredentialProvider.dll"
if (Test-Path $builtDll) {
    Copy-Item -Path (Join-Path $SourceDir "*.dll") -Destination $dir -Force
    Write-Host "Copied DLLs from $SourceDir to $dir"
}

if (-not (Test-Path $DllPath)) {
    Write-Error "DLL not found at $DllPath. Build the project first."
    Write-Host ""
    Write-Host "Build instructions:"
    Write-Host "  1. dotnet build -c Release (in desktop/crates/credential-provider)"
    Write-Host "  2. Re-run this script; it copies bin\Release\net48\*.dll to $dir"
    exit 1
}

# Register COM
$regPath = "Registry::HKEY_CLASSES_ROOT\CLSID\$clsid"
New-Item -Path $regPath -Force | Out-Null
Set-ItemProperty -Path $regPath -Name "(Default)" -Value $name

$inprocPath = "$regPath\InprocServer32"
New-Item -Path $inprocPath -Force | Out-Null
Set-ItemProperty -Path $inprocPath -Name "(Default)" -Value $DllPath
Set-ItemProperty -Path $inprocPath -Name "ThreadingModel" -Value "Apartment"

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
