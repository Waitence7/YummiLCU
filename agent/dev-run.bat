@echo off
REM 개발용: 빌드 없이 소스에서 바로 실행 (코드 수정 후 창 닫고 다시 실행)
cd /d "%~dp0YummiLcu.Agent"
echo dotnet run (dev)...
dotnet run -c Debug
pause
