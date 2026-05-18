@echo off
REM 배포 zip 만들고 version.json 안내 (수동으로 VM/GitHub에 올림)
setlocal
cd /d "%~dp0"
call build-slim.bat
if errorlevel 1 exit /b 1
echo.
echo [다음 단계]
echo 1. publish 폴더를 zip
echo 2. VM: /var/www/agent/YummiAgent.zip + deploy/agent-version.json version 올리기
echo 3. Relay 재시작 불필요 (version.json만 바뀌면 됨)
echo.
pause
