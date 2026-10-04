#!/usr/bin/env powershell
<#
.SYNOPSIS
    Builds the WristKey Credential Provider DLL for Windows.

.DESCRIPTION
    This script builds the native x64 Credential Provider DLL using MSBuild.
    It requires Visual Studio 2019/2022 with C++ Desktop workload installed.

.PARAMETER Configuration
    Build configuration: Debug or Release (default: Release)

.PARAMETER Platform
    Target platform: x64 (default: x64)

.PARAMETER OutputPath
    Optional custom output directory for the built DLL

.EXAMPLE
    .\build-credential-provider.ps1

.EXAMPLE
    .\build-credential-provider.ps1 -Configuration Release -Platform x64
#>

param(
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Release',
    
    [ValidateSet('x64')]
    [string]$Platform = 'x64',
    
    [string]$OutputPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Get script directory
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
$ProjectDir = Join-Path (Join-Path $ScriptDir '..') '..\windows-credential-provider'
$ProjectDir = Resolve-Path $ProjectDir

Write-Host "Building WristKey Credential Provider..." -ForegroundColor Cyan
Write-Host "Project directory: $ProjectDir" -ForegroundColor Gray
Write-Host "Configuration: $Configuration" -ForegroundColor Gray
Write-Host "Platform: $Platform" -ForegroundColor Gray

# Find MSBuild
$MsBuildPath = ${env:MSBuild} # Check if MSBuild is in PATH
if (-not $MsBuildPath) {
    # Try common locations
    $vsPaths = @(
        "${env:ProgramFiles}\Microsoft Visual Studio\2022\Community\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2022\Professional\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2022\Enterprise\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2019\Community\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2019\Professional\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2019\Enterprise\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2019\Community\MSBuild\Current\Bin\MSBuild.exe"
    )
    
    foreach ($path in $vsPaths) {
        if (Test-Path $path) {
            $MsBuildPath = $path
            break
        }
    }
}

if (-not $MsBuildPath) {
    Write-Error "MSBuild not found. Please install Visual Studio with C++ Desktop workload or set MSBuild environment variable."
    exit 1
}

Write-Host "Using MSBuild: $MsBuildPath" -ForegroundColor Gray

# Find the project file (native C++ credential provider)
$ProjectFile = Get-ChildItem -Path $ProjectDir -Filter '*.vcxproj' -Recurse | Select-Object -First 1
if (-not $ProjectFile) {
    Write-Error "No .vcxproj file found in $ProjectDir"
    exit 1
}

Write-Host "Building project: $($ProjectFile.FullName)" -ForegroundColor Gray

# Build arguments
$MsBuildArgs = @(
    $ProjectFile.FullName,
    "/p:Configuration=$Configuration",
    "/p:Platform=$Platform",
    "/p:TargetName=WristKeyCredentialProvider",
    "/verbosity:minimal",
    "/nologo",
    "/maxcpucount"
)

# Execute MSBuild
$process = Start-Process -FilePath $MsBuildPath -ArgumentList $MsBuildArgs -Wait -PassThru -NoNewWindow
if ($process.ExitCode -ne 0) {
    Write-Error "Build failed with exit code $($process.ExitCode)"
    exit $process.ExitCode
}

# Find the built DLL
$BuiltDll = Join-Path $ProjectDir "build\$Configuration\WristKeyCredentialProvider.dll"
if (-not (Test-Path $BuiltDll)) {
    # Try alternative output paths
    $BuiltDll = Get-ChildItem -Path $ProjectDir -Filter 'WristKeyCredentialProvider.dll' -Recurse | 
                Where-Object { $_.FullName -like "*$Configuration*" } | 
                Select-Object -First 1
}

if (-not $BuiltDll -or -not (Test-Path $BuiltDll.FullName)) {
    Write-Error "Built DLL not found at expected location"
    exit 1
}

Write-Host "Build successful!" -ForegroundColor Green
Write-Host "DLL location: $($BuiltDll.FullName)" -ForegroundColor Cyan

# Copy to output path if specified
if ($OutputPath) {
    $OutputDir = Resolve-Path $OutputPath
    $DestPath = Join-Path $OutputDir "WristKeyCredentialProvider.dll"
    Copy-Item -Path $BuiltDll.FullName -Destination $DestPath -Force
    Write-Host "Copied to: $DestPath" -ForegroundColor Green
}

# Also copy to standard install location if running as admin
$InstallPath = "C:\Program Files\WristKey\WristKeyCredentialProvider.dll"
$IsAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if ($IsAdmin) {
    $InstallDir = Split-Path -Parent $InstallPath
    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }
    Copy-Item -Path $BuiltDll.FullName -Destination $InstallPath -Force
    Write-Host "Installed to: $InstallPath" -ForegroundColor Green
} else {
    Write-Host "Note: Run as Administrator to auto-install to $InstallPath" -ForegroundColor Yellow
}

Write-Host "`nNext steps:" -ForegroundColor Cyan
Write-Host "1. Run register.ps1 as Administrator to register the credential provider" -ForegroundColor Gray
Write-Host "2. Ensure WristKey daemon is running before lock screen" -ForegroundColor Gray
Write-Host "3. Restart computer or run 'shutdown /r /t 0' to apply changes" -ForegroundColor Gray