#requires -RunAsAdministrator
param(
    [string]$Dll = (Join-Path $PSScriptRoot 'build\Release\WristKeyCredentialProvider.dll')
)

$ErrorActionPreference = 'Stop'

if (-not [Environment]::Is64BitOperatingSystem) {
    throw 'WristKey Credential Provider requires 64-bit Windows.'
}

$dll = (Resolve-Path $Dll).Path
$regsvr32 = Join-Path $env:WINDIR 'System32\regsvr32.exe'
$clsid = '{7E1B7B8A-4C8B-4C2F-9D8A-8D3A7F2E51A1}'

$clsidKey = "HKLM:\SOFTWARE\Classes\CLSID\$clsid"
$inprocKey = "$clsidKey\InprocServer32"
$cpKey = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\$clsid"

Write-Host "[*] DLL: $dll"
Write-Host '[*] Registering WristKey provider with regsvr32...'

& $regsvr32 /s $dll
if ($LASTEXITCODE -ne 0) {
    throw "regsvr32 registration failed with exit code $LASTEXITCODE"
}

try {
    if (-not (Test-Path $clsidKey)) { throw "CLSID key missing: $clsidKey" }
    if (-not (Test-Path $inprocKey)) { throw "InprocServer32 key missing: $inprocKey" }
    if (-not (Test-Path $cpKey)) { throw "Credential Provider key missing: $cpKey" }

    $serverPath = (Get-ItemProperty -Path $inprocKey -Name '(default)').'(default)'
    $threadingModel = (Get-ItemProperty -Path $inprocKey -Name 'ThreadingModel').ThreadingModel
    $providerName = (Get-ItemProperty -Path $cpKey -Name '(default)').'(default)'

    if ([System.IO.Path]::GetFullPath($serverPath) -ne [System.IO.Path]::GetFullPath($dll)) {
        throw "InprocServer32 points to '$serverPath' instead of '$dll'"
    }
    if ($threadingModel -ne 'Apartment') {
        throw "Unexpected ThreadingModel: '$threadingModel'"
    }
    if ($providerName -ne 'WristKey Credential Provider') {
        throw "Unexpected provider name: '$providerName'"
    }

    Write-Host '[+] Registration round-trip checks passed.'
    Write-Host '[+] CLSID, InprocServer32, DLL path, ThreadingModel and Credential Provider registration are correct.'
}
finally {
    Write-Host '[*] Immediately unregistering WristKey provider...'
    & $regsvr32 /s /u $dll
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "regsvr32 unregister failed with exit code $LASTEXITCODE"
    }

    if ((Test-Path $clsidKey) -or (Test-Path $cpKey)) {
        Write-Warning 'Some provider registry keys remain after unregister; inspect manually.'
    } else {
        Write-Host '[+] Provider registration removed.'
    }
}
