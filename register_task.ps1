$exePath = "C:\Users\ryokh\Data\Coding\auto-rename-notary-file\target\release\notaris_ocr_cepat.exe"
$workDir = "C:\Users\ryokh\Data\Coding\auto-rename-notary-file"
$action = New-ScheduledTaskAction -Execute $exePath -WorkingDirectory $workDir
$trigger = New-ScheduledTaskTrigger -AtLogOn
# Delay 15 detik setelah logon agar Google Drive sempat inisialisasi drive G:\
$trigger.Delay = 'PT15S'

$settings = New-ScheduledTaskSettingsSet `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries `
    -RestartCount 5 `
    -RestartInterval (New-TimeSpan -Minutes 1) `
    -ExecutionTimeLimit (New-TimeSpan -Days 365)

$principal = New-ScheduledTaskPrincipal -UserId "ryokh" -LogonType Interactive

Register-ScheduledTask -TaskName "AutoRenameNotaris" -Action $action -Trigger $trigger -Settings $settings -Principal $principal -Description "Otomatis rename dokumen scan notaris via OCR" -Force
Start-ScheduledTask -TaskName "AutoRenameNotaris"

Write-Host ""
Write-Host "=================================================================" -ForegroundColor Green
Write-Host "  SUKSES! Tugas 'AutoRenameNotaris' Berhasil Didaftarkan" -ForegroundColor Green
Write-Host "=================================================================" -ForegroundColor Green
Write-Host "  Program sekarang aktif berjalan di latar belakang dan akan"
Write-Host "  otomatis aktif setiap kali Anda menyalakan komputer / login."
Write-Host "=================================================================" -ForegroundColor Green
Write-Host ""
