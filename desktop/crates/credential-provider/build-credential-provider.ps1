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
$RepoRoot = Join-Path (Join-Path $ScriptDir '..') '..'
$RepoRoot = Join-Path $RepoRoot '..\..'
$ProjectDir = Join-Path $RepoRoot 'windows-credential-provider'
$ProjectDir = Resolve-Path $ProjectDir

Write-Host "Building WristKey Credential Provider..." -ForegroundColor Cyan
Write-Host "Project directory: $ProjectDir" -ForegroundColor Gray
Write-Host "Configuration: $Configuration" -ForegroundColor Gray
Write-Host "Platform: $Platform" -ForegroundColor Gray

# Find MSBuild - PRIORITIZE VS MSBuild over .NET Framework MSBuild
$MsBuildPath = ${env:MSBuild} # Check if MSBuild is in PATH
if (-not $MsBuildPath) {
    # Try common locations - search both ProgramFiles and ProgramFiles(x86)
    # VS typically installs to ProgramFiles(x86) even on 64-bit systems
    $vsPaths = @(
        # VS 2026
        "${env:ProgramFiles}\Microsoft Visual Studio\2026\Community\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2026\Professional\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2026\Enterprise\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2026\BuildTools\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2026\Community\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2026\Professional\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2026\Enterprise\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2026\BuildTools\MSBuild\Current\Bin\MSBuild.exe",
        # VS 2022 (preferred - has C++ tools)
        "${env:ProgramFiles}\Microsoft Visual Studio\2022\Community\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2022\Professional\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2022\Enterprise\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\Community\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\Professional\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\Enterprise\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe",
        # VS 2019
        "${env:ProgramFiles}\Microsoft Visual Studio\2019\Community\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2019\Professional\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2019\Enterprise\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2019\BuildTools\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2019\Community\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2019\Professional\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2019\Enterprise\MSBuild\Current\Bin\MSBuild.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2019\BuildTools\MSBuild\Current\Bin\MSBuild.exe",
        # VS 2017
        "${env:ProgramFiles}\Microsoft Visual Studio\2017\Community\MSBuild\15.0\Bin\MSBuild.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2017\Professional\MSBuild\15.0\Bin\MSBuild.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2017\Enterprise\MSBuild\15.0\Bin\MSBuild.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2017\Community\MSBuild\15.0\Bin\MSBuild.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2017\Professional\MSBuild\15.0\Bin\MSBuild.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2017\Enterprise\MSBuild\15.0\Bin\MSBuild.exe",
        # .NET Framework MSBuild (fallback - may NOT have C++ tools!)
        "${env:windir}\Microsoft.NET\Framework64\v4.0.30319\MSBuild.exe",
        "${env:windir}\Microsoft.NET\Framework\v4.0.30319\MSBuild.exe"
    )
    
    foreach ($path in $vsPaths) {
        if (Test-Path $path) {
            $MsBuildPath = $path
            break
        }
    }
}

# Validate C++ tools are available
$HasCppTools = $false
if ($MsBuildPath -and $MsBuildPath -notlike "*Microsoft.NET*") {
    # Extract VS install directory from MSBuild path
    # Example: C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe
    $vsRoot = Split-Path (Split-Path (Split-Path $MsBuildPath))  # Go up from Bin to MSBuild\Current
    $vsRoot = Split-Path $vsRoot  # Go up to MSBuild
    # Now vsRoot is like: C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\MSBuild
    
    # Search for Microsoft.Cpp.Default.props in various possible locations
    $searchPaths = @(
        "$vsRoot\Microsoft\VC\v170",
        "$vsRoot\Microsoft\VC\v160",
        "$vsRoot\Microsoft\VC\v150",
        "$vsRoot\Microsoft\VC\v143",
        "$vsRoot\Microsoft\VC\v142",
        # Try going up to VS install root and checking
        "$((Split-Path $vsRoot))\..\..\..\MSBuild\Microsoft\VC\v170"
    )
    
    foreach ($searchPath in $searchPaths) {
        $cppPropsPath = Join-Path $searchPath "Microsoft.Cpp.Default.props"
        if (Test-Path $cppPropsPath) {
            $HasCppTools = $true
            break
        }
    }
}

if (-not $HasCppTools) {
    Write-Host ""
    Write-Host "ERROR: Visual C++ Build Tools not found!" -ForegroundColor Red
    Write-Host "The credential provider is a C++ project and requires the C++ compiler and libraries." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "You have Visual Studio installed, but the 'Desktop development with C++' workload is missing." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "To fix this:" -ForegroundColor Cyan
    Write-Host "  1. Open 'Visual Studio Installer' (search in Start menu)" -ForegroundColor Gray
    Write-Host "  2. Find your installation (Build Tools 2022 or Community 2026)" -ForegroundColor Gray
    Write-Host "  3. Click 'Modify'" -ForegroundColor Gray
    Write-Host "  4. Select 'Desktop development with C++' workload" -ForegroundColor Green
    Write-Host "  5. Click 'Modify' to install" -ForegroundColor Gray
    Write-Host ""
    Write-Host "Required components (auto-selected with workload):" -ForegroundColor Cyan
    Write-Host "  [Checked] MSVC v143 - VS 2022 C++ x64/x86 build tools" -ForegroundColor Green
    Write-Host "  [Checked] Windows 10 SDK (or Windows 11 SDK)" -ForegroundColor Green
    Write-Host "  [Checked] Microsoft.Cpp.Default.props (MSBuild C++ support)" -ForegroundColor Green
    Write-Host ""
    Write-Host "After installation, restart PowerShell and run this script again." -ForegroundColor Cyan
    exit 1
}

Write-Host "Using MSBuild: $MsBuildPath" -ForegroundColor Gray
Write-Host "C++ tools: Found" -ForegroundColor Green

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