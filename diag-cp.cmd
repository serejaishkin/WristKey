@echo off
setlocal EnableExtensions
rem diag-cp.cmd -- why is the WristKey tile not on the lock screen? Run as admin; prints everything.
net session >nul 2>&1
if errorlevel 1 (
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)
set "CLSID={7E1B7B8A-4C8B-4C2F-9D8A-8D3A7F2E51A1}"
set "OUT=%ProgramData%\WristKey\diag-cp.txt"
if not exist "%ProgramData%\WristKey" mkdir "%ProgramData%\WristKey"
echo Writing report to %OUT%
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
 "$ErrorActionPreference='Continue'; $clsid='%CLSID%'; $r=@();" ^
 "function A($t){ $script:r += $t; Write-Host $t }" ^
 "A ('Windows: ' + (Get-CimInstance Win32_OperatingSystem).Caption + ' build ' + [Environment]::OSVersion.Version);" ^
 "$dll=Join-Path $env:SystemRoot 'System32\WristKeyCredentialProvider.dll';" ^
 "if(Test-Path $dll){ $b=[IO.File]::ReadAllBytes($dll); $pe=[BitConverter]::ToInt32($b,0x3C); $m=[BitConverter]::ToUInt16($b,$pe+4); A ('DLL: present, machine=0x{0:X} ({1}), size={2}, modified={3}' -f $m, $(if($m -eq 0x8664){'x64 OK'}else{'NOT x64!'}), $b.Length, (Get-Item $dll).LastWriteTime) } else { A 'DLL: MISSING in System32' }" ^
 "$cp='HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\'+$clsid; A ('CP key: ' + (Test-Path $cp));" ^
 "$ip='HKLM:\SOFTWARE\Classes\CLSID\'+$clsid+'\InprocServer32'; if(Test-Path $ip){ $k=Get-Item $ip; A ('InprocServer32: ' + $k.GetValue('') + '  threading=' + $k.GetValue('ThreadingModel')) } else { A 'InprocServer32: MISSING' }" ^
 "Get-ChildItem 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers' | ForEach-Object { A ('  registered CP: ' + $_.PSChildName + '  ' + $_.GetValue('')) };" ^
 "$pl='HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\PasswordLess\Device'; if(Test-Path $pl){ $v=(Get-ItemProperty $pl).DevicePasswordLessBuildVersion; A ('Hello-only sign-in (DevicePasswordLessBuildVersion): ' + $v + '   <- 2 hides password-style tiles; set 0 to turn off') } else { A 'Hello-only sign-in key: absent (off)' }" ^
 "$ex='HKLM:\SOFTWARE\Policies\Microsoft\Windows\System'; $e=(Get-ItemProperty $ex -ErrorAction SilentlyContinue).ExcludeCredentialProviders; A ('ExcludeCredentialProviders policy: ' + $(if($e){$e}else{'(none)'}));" ^
 "A ('Service: ' + ((Get-Service WristKey -ErrorAction SilentlyContinue) | ForEach-Object { $_.Status }));" ^
 "A 'Pipe \\.\pipe\wristkey exists: ' + (Test-Path '\\.\pipe\wristkey');" ^
 "A '--- LogonUI / provider crashes (Application log) ---';" ^
 "Get-WinEvent -FilterHashtable @{LogName='Application'; Id=1000,1001} -MaxEvents 40 -ErrorAction SilentlyContinue | Where-Object { $_.Message -match 'LogonUI|WristKey' } | Select-Object -First 5 | ForEach-Object { A ($_.TimeCreated.ToString() + ' ' + ($_.Message -split \"`n\")[0..3] -join ' ') };" ^
 "$r | Out-File -Encoding utf8 '%OUT%'"
echo.
pause
