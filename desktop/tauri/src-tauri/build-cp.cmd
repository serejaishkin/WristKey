@echo off
rem Builds the Credential Provider DLL before bundling. %~dp0 is this
rem script's directory (src-tauri), so the path works from any cwd.
set CSPROJ=%~dp0..\..\crates\credential-provider\WristKeyCredentialProvider.csproj
if not exist "%CSPROJ%" set CSPROJ=%~dp0..\crates\credential-provider\WristKeyCredentialProvider.csproj
dotnet build -c Release "%CSPROJ%"
exit /b %errorlevel%
