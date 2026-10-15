@echo off
title Pasang Auto-Rename ke Task Scheduler
:: Memeriksa izin Administrator
net session >nul 2>&1
if %errorLevel% == 0 (
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0register_task.ps1"
    pause
) else (
    echo Meminta izin Administrator...
    powershell.exe -Command "Start-Process cmd -ArgumentList '/c \"\"%~f0\"\"' -Verb RunAs"
)
