@echo off
rem XQ bench: GPUI vs egui. Double-click to run; extra args go to bench-windows.ps1, e.g. run-bench.cmd -Quick
chcp 65001 >nul
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0bench-windows.ps1" %*
echo.
pause
