@echo off
rem ---- MSVC env wrapper: avoid Git usr/bin/link.exe shadowing and broken vcvars64 ----
set "MSVC=D:\Env\VisualStudioCC\core\VC\Tools\MSVC\14.51.36231"
set "SDK=C:\Program Files (x86)\Windows Kits\10"
set "SDKVER=10.0.26100.0"
set "PATH=%MSVC%\bin\Hostx64\x64;%PATH%"
set "INCLUDE=%MSVC%\include;%SDK%\Include\%SDKVER%\ucrt;%SDK%\Include\%SDKVER%\um;%SDK%\Include\%SDKVER%\shared;%SDK%\Include\%SDKVER%\winrt"
set "LIB=%MSVC%\lib\x64;%SDK%\Lib\%SDKVER%\ucrt\x64;%SDK%\Lib\%SDKVER%\um\x64"
cd /d "%~dp0"
pushd src-tauri
cargo build --bin lightr-ask-mcp
if errorlevel 1 exit /b %errorlevel%
popd
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\smoke-lightr-ask-mcp.ps1 -Configuration debug
if errorlevel 1 exit /b %errorlevel%
pnpm tauri dev %*
