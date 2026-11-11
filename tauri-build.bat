@echo off
rem Windows 手动入口：统一转到跨平台准备脚本
rem （Vite 构建 + lightr-ask-mcp 编译/暂存 + smoke 测试）。
node "%~dp0scripts\prepare-tauri-build.mjs"
if errorlevel 1 exit /b %errorlevel%
