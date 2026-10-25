#Requires -RunAsAdministrator

Stop-Service WristKey -Force -ErrorAction SilentlyContinue

$clsids = @(
    "{7E1B7B8A-4C8B-4C2F-9D8A-8D3A7F2E51A1}",
    "{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}",
    "{5FD3D285-0DD9-4362-8855-E0ABAACD4AF6}"
)

$cpHives = @(
    "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers",
    "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers"
)
$classHives = @(
    "HKLM:\SOFTWARE\Classes\CLSID",
    "HKCU:\SOFTWARE\Classes\CLSID"
)

foreach ($cpPath in $cpHives) {
    foreach ($clsid in $clsids) {
        $cpKey = Join-Path $cpPath $clsid
        if (Test-Path $cpKey) { Remove-Item $cpKey -Recurse -Force -ErrorAction SilentlyContinue }
    }
}
foreach ($classPath in $classHives) {
    foreach ($clsid in $clsids) {
        $clKey = Join-Path $classPath $clsid
        if (Test-Path $clKey) { Remove-Item $clKey -Recurse -Force -ErrorAction SilentlyContinue }
    }
}

$candidates = @(
    "C:\Windows\System32\WristKeyCredentialProvider.dll",
    "C:\Program Files\WristKey\WristKeyCredentialProvider.dll",
    "$PSScriptRoot\WristKeyCredentialProvider.dll"
)
$dll = $null
foreach ($p in $candidates) { if (Test-Path $p) { $dll = $p; break } }

if (-not $dll) {
    Write-Error "WristKeyCredentialProvider.dll not found."
    exit 1
}

Copy-Item $dll "C:\Windows\System32\WristKeyCredentialProvider.dll" -Force
& "C:\Windows\System32\regsvr32.exe" /s "C:\Windows\System32\WristKeyCredentialProvider.dll"

Write-Host "Done. Reboot PC."
