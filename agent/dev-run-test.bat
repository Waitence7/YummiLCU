@echo off
REM 롤 클라이언트 없이 테스트 모드로 실행
cd /d "%~dp0YummiLcu.App"
echo dotnet run --test (UI test mode)...
dotnet run -c Debug -- --test
pause
