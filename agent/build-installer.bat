@echo off
setlocal EnableExtensions
cd /d "%~dp0"

echo [1/3] Portable publish...
cd YummiLcu.App
dotnet publish -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -p:EnableCompressionInSingleFile=true
if errorlevel 1 goto fail
cd ..

if exist "agent.json" (
  copy /Y "agent.json" "YummiLcu.App\bin\Release\net8.0-windows\win-x64\publish\agent.json" >nul
) else (
  copy /Y "agent.json.example" "YummiLcu.App\bin\Release\net8.0-windows\win-x64\publish\agent.json" >nul
)

for /f "usebackq tokens=*" %%V in (`powershell -NoProfile -Command "[xml]$x=Get-Content 'YummiLcu.App\YummiLcu.App.csproj'; $x.Project.PropertyGroup.Version"`) do set APP_VER=%%V
if not defined APP_VER set APP_VER=0.0.0

set "ISCC=%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe"
if not exist "%ISCC%" set "ISCC=%ProgramFiles%\Inno Setup 6\ISCC.exe"
if not exist "%ISCC%" (
  echo Inno Setup 6 not found. Install: https://jrsoftware.org/isdl.php
  goto fail
)

echo [2/3] Inno Setup compile (version %APP_VER%)...
"%ISCC%" /DAppVersion=%APP_VER% "installer\YummiAgent.iss"
if errorlevel 1 goto fail

echo [3/3] Zip portable (auto-update)...
set "PUB=YummiLcu.App\bin\Release\net8.0-windows\win-x64\publish"
powershell -NoProfile -Command "Compress-Archive -Path '%PUB%\*' -DestinationPath 'installer\output\YummiAgent-win-x64-portable.zip' -Force"

echo.
echo OK
echo   Installer: installer\output\YummiAgent-Setup-%APP_VER%.exe
echo   Zip:       installer\output\YummiAgent-win-x64-portable.zip
start explorer "%~dp0installer\output"
goto end

:fail
echo FAILED.
exit /b 1

:end
pause
