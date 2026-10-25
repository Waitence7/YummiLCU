@echo off
setlocal EnableExtensions

set "INSTALL_DIR=%~dp0"
set "INSTALL_PS1=%INSTALL_DIR%Install-YummiGameBar.ps1"

rem Windows Explorer may run only this .cmd from inside a ZIP by extracting a
rem single file to a temporary folder. In that case the companion files are
rem not present, so fail with a clear instruction instead of a PowerShell path
rem error.
if not exist "%INSTALL_PS1%" (
  echo.
  echo [Yummi Game Bar] Installation files are incomplete.
  echo.
  echo This usually happens when Install-YummiGameBar.cmd is opened directly
  echo from inside the ZIP file.
  echo.
  echo 1. Close this window.
  echo 2. Right-click Yummi-GameBar-Overlay-0.8.0.zip and choose "Extract All...".
  echo 3. Open the extracted folder.
  echo 4. Run Install-YummiGameBar.cmd again.
  echo.
  pause
  exit /b 2
)

if not exist "%INSTALL_DIR%Yummi-GameBar-Overlay.cer" (
  echo [Yummi Game Bar] Yummi-GameBar-Overlay.cer is missing.
  echo Please extract the entire ZIP before installing.
  pause
  exit /b 2
)

set "PACKAGE_FOUND="
for %%F in ("%INSTALL_DIR%*.msix" "%INSTALL_DIR%*.appx") do (
  if exist "%%~fF" set "PACKAGE_FOUND=1"
)
if not defined PACKAGE_FOUND (
  echo [Yummi Game Bar] The .msix/.appx package is missing.
  echo Please extract the entire ZIP before installing.
  pause
  exit /b 2
)

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%INSTALL_PS1%" -Force
set "INSTALL_EXIT=%ERRORLEVEL%"

if not "%INSTALL_EXIT%"=="0" (
  echo.
  echo Installation failed. Check the message above.
  pause
  exit /b %INSTALL_EXIT%
)

echo.
echo Installation completed. Press Win+G and open the Yummi LCU widget.
pause
