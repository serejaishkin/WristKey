#requires -RunAsAdministrator
param(
    [ValidateSet('Install','Uninstall')]
    [string]$Action = 'Install'
)

$ErrorActionPreference = 'Stop'

if (-not [Environment]::Is64BitOperatingSystem) {
    throw 'WristKey requires 64-bit Windows for this control test.'
}

$sampleRoot = Join-Path $PSScriptRoot '.microsoft-v2-control'
$sampleDir = Join-Path $sampleRoot 'CredentialProvider\cpp'
$zipPath = Join-Path $env:TEMP 'Windows-classic-samples-main.zip'
$extractRoot = Join-Path $env:TEMP 'Windows-classic-samples-main'
$system32 = Join-Path $env:WINDIR 'System32'
$dllTarget = Join-Path $system32 'SampleV2CredentialProvider.dll'
$clsid = '{5FD3D285-0DD9-4362-8855-E0ABAA CD4AF6}'.Replace(' ','')

function Get-MSBuild {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) {
        throw 'vswhere.exe not found. Install Visual Studio 2022 / Build Tools with Desktop C++ workload.'
    }

    $installationPath = & $vswhere -latest -products * -requires Microsoft.Component.MSBuild -property installationPath
    if (-not $installationPath) {
        throw 'No Visual Studio installation with MSBuild was found.'
    }

    $msbuild = Join-Path $installationPath 'MSBuild\Current\Bin\MSBuild.exe'
    if (-not (Test-Path $msbuild)) {
        throw "MSBuild not found: $msbuild"
    }
    return $msbuild
}

function Prepare-Sample {
    if (Test-Path $sampleDir) { return }

    New-Item -ItemType Directory -Force -Path $sampleRoot | Out-Null

    $url = 'https://github.com/microsoft/Windows-classic-samples/archive/refs/heads/main.zip'
    Write-Host '[*] Downloading Microsoft Windows-classic-samples...'
    Invoke-WebRequest -Uri $url -OutFile $zipPath

    if (Test-Path $extractRoot) { Remove-Item $extractRoot -Recurse -Force }
    Expand-Archive -LiteralPath $zipPath -DestinationPath $env:TEMP -Force

    $sourceRoot = Join-Path $env:TEMP 'Windows-classic-samples-main'
    $sourceDir = Join-Path $sourceRoot 'Samples\CredentialProvider'
    if (-not (Test-Path (Join-Path $sourceDir 'cpp\SampleV2CredentialProvider.vcxproj'))) {
        throw 'Microsoft Credential Provider sample was not found in the downloaded archive.'
    }

    Copy-Item -LiteralPath $sourceDir -Destination $sampleRoot -Recurse -Force
}

if ($Action -eq 'Uninstall') {
    Prepare-Sample
    $unregisterReg = Join-Path $sampleDir 'Unregister.reg'
    if (Test-Path $unregisterReg) {
        & reg.exe import $unregisterReg
        if ($LASTEXITCODE -ne 0) { throw "Microsoft sample unregister failed with exit code $LASTEXITCODE" }
    }

    if (Test-Path $dllTarget) {
        Remove-Item $dllTarget -Force -ErrorAction SilentlyContinue
    }

    Write-Host '[+] Microsoft V2 Credential Provider control removed.'
    exit 0
}

Prepare-Sample

$msbuild = Get-MSBuild
$project = Join-Path $sampleDir 'cpp\SampleV2CredentialProvider.vcxproj'

Write-Host '[*] Building Microsoft V2 Credential Provider control (Release|x64)...'
& $msbuild $project /m /p:Configuration=Release /p:Platform=x64
if ($LASTEXITCODE -ne 0) {
    throw "Microsoft sample build failed with exit code $LASTEXITCODE"
}

$dll = Get-ChildItem -Path (Split-Path $project) -Filter 'SampleV2CredentialProvider.dll' -Recurse -File |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
if (-not $dll) {
    throw 'Built SampleV2CredentialProvider.dll was not found.'
}

Write-Host "[*] Copying control DLL to $system32..."
Copy-Item -LiteralPath $dll.FullName -Destination $dllTarget -Force

$registerReg = Join-Path $sampleDir 'register.reg'
if (-not (Test-Path $registerReg)) {
    throw "Microsoft register.reg not found: $registerReg"
}

Write-Host '[*] Registering Microsoft control provider...'
& reg.exe import $registerReg
if ($LASTEXITCODE -ne 0) {
    throw "reg.exe import failed with exit code $LASTEXITCODE"
}

$cpKey = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\$clsid"
$clsidKey = "HKLM:\SOFTWARE\Classes\CLSID\$clsid\InprocServer32"
if (-not (Test-Path $cpKey) -or -not (Test-Path $clsidKey)) {
    throw 'Microsoft control provider registry registration is incomplete.'
}

Write-Host ''
Write-Host '[+] Microsoft V2 Credential Provider control is installed.'
Write-Host '[+] Sign out/reboot and check the Windows sign-in screen.'
Write-Host '[+] Expected: a tile named "Sample Credential Provider".'
Write-Host ''
Write-Host 'If this tile appears, Windows accepts the provider architecture on this machine.'
Write-Host 'Then we will transplant this exact V2 architecture into WristKey and keep the WristKey BLE/named-pipe backend.'
