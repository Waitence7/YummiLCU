@echo off
REM 개발용: WPF 앱 소스에서 바로 실행
cd /d "%~dp0YummiLcu.App"
echo dotnet run (dev)...
dotnet run -c Debug
pause
