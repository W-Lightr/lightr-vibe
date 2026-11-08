@echo off
rem Build the UI and the exact companion binary that Tauri bundles as a resource.
call pnpm build
if errorlevel 1 exit /b %errorlevel%
pushd src-tauri
cargo build --release --bin lightr-ask-mcp
if errorlevel 1 exit /b %errorlevel%
copy /Y "target\release\lightr-ask-mcp.exe" "binaries\lightr-ask-mcp.exe" >nul
if errorlevel 1 exit /b %errorlevel%
popd
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\smoke-lightr-ask-mcp.ps1 -Configuration release -RequireStagedResource
if errorlevel 1 exit /b %errorlevel%
