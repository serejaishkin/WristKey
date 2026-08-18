# Run from an elevated 64-bit PowerShell after building WristKeyCredentialProvider.dll.
$ErrorActionPreference = 'Stop'
$clsid = '{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}'
$dll = Join-Path $PSScriptRoot 'bin\WristKeyCredentialProvider.dll'
$regasm = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\RegAsm.exe'
if (!(Test-Path $dll)) { throw "DLL not found: $dll" }
if (!(Test-Path $regasm)) { throw "64-bit RegAsm not found: $regasm" }

& $regasm $dll /codebase /tlb | Out-Host
$cp = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\' + $clsid
New-Item -Path $cp -Force | Out-Null
Set-ItemProperty -Path $cp -Name '(default)' -Value 'WristKey Credential Provider'

Write-Host 'WristKey Credential Provider registered.'
Write-Host 'Do not reboot yet. First verify the DLL is x64 and restart LogonUI by locking Windows.'
