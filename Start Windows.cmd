@echo off
cd /d "%~dp0"
if exist "%~dp0ane-console.exe" (
  start "" "%~dp0ane-console.exe"
  exit /b
)
if exist "%~dp0local\target\debug\ane-console.exe" (
  start "" "%~dp0local\target\debug\ane-console.exe"
  exit /b
)
echo Build the Rust application first: cargo build -p ane-app --bin ane-console
pause
