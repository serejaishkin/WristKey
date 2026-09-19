#requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'

$nativeClsid = '{7E1B7B8A-4C8B-4C2F-9D8A-8D3A7F2E51A1}'
$legacyClsid = '{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}'
$system32 = Join-Path $env:WINDIR 'System32'
$nativeDll = Join-Path $system32 'WristKeyCredentialProvider.dll'
$regsvr32 = Join-Path $system32 'regsvr32.exe'

function Remove-Tree([string]$path) {
    if (Test-Path -LiteralPath $path) {
        Remove-Item -LiteralPath $path -Recurse -Force -ErrorAction SilentlyContinue
    }
}

foreach ($clsid in @($nativeClsid, $legacyClsid)) {
    Remove-Tree "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\$clsid"
    Remove-Tree "Registry::HKEY_CLASSES_ROOT\CLSID\$clsid"
}

if (Test-Path -LiteralPath $nativeDll) {
    try { & $regsvr32 '/u' '/s' $nativeDll } catch {}
    Remove-Item -LiteralPath $nativeDll -Force -ErrorAction SilentlyContinue
}

Write-Host 'WristKey Credential Provider registration removed.'
Write-Host 'Built-in Windows password/PIN providers are untouched by this script.'
Write-Host 'Restart Windows before testing Sign-in options again.'
