@echo off
setlocal EnableExtensions
cd /d "%~dp0"
cd YummiLcu.Agent
echo [slim] dotnet publish (needs .NET 8 Desktop Runtime on PC)...
dotnet publish -c Release -r win-x64 --self-contained false -p:PublishSingleFile=true
if errorlevel 1 goto fail
set "OUT=bin\Release\net8.0-windows\win-x64\publish"
copy /Y "..\agent.json" "%OUT%\agent.json" >nul
echo.
echo Build OK (slim, ~5-15 MB)
echo Run: %CD%\%OUT%\YummiLcu.Agent.exe
echo Zip for friends: right-click publish folder -^> Send to -^> Compressed folder
start explorer "%CD%\%OUT%"
goto end
:fail
echo FAILED. SDK 9+ / .NET 8 Desktop Runtime 확인: https://dotnet.microsoft.com/download/dotnet/8.0
:end
pause
