@echo off
setlocal EnableExtensions

set "INSTALL_DIR=%~dp0"
set "INSTALL_PS1=%INSTALL_DIR%Install-YummiGameBar.ps1"
set "LOG_FILE=%TEMP%\Yummi-GameBar-install.log"

if not exist "%INSTALL_PS1%" exit /b 2
if not exist "%INSTALL_DIR%Yummi-GameBar-Overlay.cer" exit /b 2

set "PACKAGE_FOUND="
for %%F in ("%INSTALL_DIR%*.msix" "%INSTALL_DIR%*.appx") do (
  if exist "%%~fF" set "PACKAGE_FOUND=1"
)
if not defined PACKAGE_FOUND exit /b 2

powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -Command ^
  "$script='%INSTALL_PS1%'; $log='%LOG_FILE%'; Remove-Item $log -Force -ErrorAction SilentlyContinue; $argLine='-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File ""' + $script + '"" -Force -LogPath ""' + $log + '""'; $p=Start-Process powershell.exe -Verb RunAs -WindowStyle Hidden -ArgumentList $argLine -Wait -PassThru; exit $p.ExitCode"

exit /b %ERRORLEVEL%
