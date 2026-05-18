@echo off
REM 빌드 + version.json 동기화 → VM에 agent-publish.sh 한 번이면 끝
setlocal
cd /d "%~dp0"
call build-portable.bat
if errorlevel 1 exit /b 1
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0..\deploy\sync-agent-version.ps1"
echo.
echo [VM 한 줄 배포]
echo   zip publish folder -^> YummiAgent-portable.zip
echo   scp YummiAgent-portable.zip ubuntu@VM:/tmp/agent.zip
echo   ssh ubuntu@VM "cd ~/Yummi/YummiLcu && ./deploy/agent-publish.sh /tmp/agent.zip"
echo.
echo GitHub Actions VM_HOST/VM_USER/VM_SSH_KEY 설정 시 main push 후 자동 배포됩니다.
pause
