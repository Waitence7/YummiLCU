@echo off
setlocal
cd /d "%~dp0"

powershell.exe -NoProfile -ExecutionPolicy Bypass -Command ^
  "$id=[Security.Principal.WindowsIdentity]::GetCurrent();" ^
  "$p=New-Object Security.Principal.WindowsPrincipal($id);" ^
  "if(-not $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){ Start-Process -FilePath '%~f0' -Verb RunAs; exit };" ^
  "& '%~dp0Install-YummiGameBar.ps1' -Force"

if errorlevel 1 (
  echo.
  echo Installation failed. Check the message above.
  pause
  exit /b 1
)

echo.
echo Installation completed. Press Win+G and open the Yummi LCU widget.
pause
