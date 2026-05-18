@echo off
REM 개발용: 파일 저장할 때마다 자동 재실행 (가장 편함)
cd /d "%~dp0YummiLcu.Agent"
echo dotnet watch run — 저장하면 자동 재시작
dotnet watch run -c Debug
pause
