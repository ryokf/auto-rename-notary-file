@echo off
title Hapus Auto-Rename dari Task Scheduler
net session >nul 2>&1
if %errorLevel% == 0 (
    powershell.exe -NoProfile -Command "Unregister-ScheduledTask -TaskName 'AutoRenameNotaris' -Confirm:$false; Write-Host 'Tugas AutoRenameNotaris berhasil dihapus.' -ForegroundColor Yellow"
    pause
) else (
    echo Meminta izin Administrator...
    powershell.exe -Command "Start-Process cmd -ArgumentList '/c \"\"%~f0\"\"' -Verb RunAs"
)
