@echo off
setlocal EnableExtensions
cd /d "%~dp0"
cd YummiLcu.Agent
echo [portable] dotnet publish (no runtime install, ~80-150 MB)...
dotnet publish -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -p:EnableCompressionInSingleFile=true
if errorlevel 1 goto fail
set "OUT=bin\Release\net8.0-windows\win-x64\publish"
if exist "..\agent.json" (copy /Y "..\agent.json" "%OUT%\agent.json" >nul) else (copy /Y "..\agent.json.example" "%OUT%\agent.json" >nul)
echo.
echo Build OK (portable)
echo Run: %CD%\%OUT%\YummiLcu.Agent.exe
start explorer "%CD%\%OUT%"
goto end
:fail
echo FAILED. Install .NET SDK: https://dotnet.microsoft.com/download
:end
pause
