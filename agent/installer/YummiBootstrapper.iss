; Yummi Agent bootstrapper — downloads latest installer from server.
#ifndef AppVersion
  #define AppVersion "1.0.0"
#endif

[Setup]
AppId={{92A30F8A-9562-47DF-8E59-9492498A40D0}
AppName=Yummi Agent Setup
AppVersion={#AppVersion}
AppVerName=Yummi Agent Setup {#AppVersion}
DefaultDirName={tmp}\YummiAgentBootstrapper
CreateAppDir=no
Uninstallable=no
OutputDir=output
OutputBaseFilename=setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
DisableWelcomePage=yes
DisableFinishedPage=yes

[Languages]
Name: "korean"; MessagesFile: "compiler:Languages\Korean.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Code]
function InitializeSetup(): Boolean;
var
  ResultCode: Integer;
  Args: String;
begin
  Args := '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command "' +
    '$ErrorActionPreference = ""Stop""; ' +
    '$ProgressPreference = ""SilentlyContinue""; ' +
    '$log = Join-Path $env:TEMP ""yummi-bootstrap.log""; ' +
    'try { ' +
    '[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; ' +
    '$m = Invoke-RestMethod -Uri ""https://yummi.duckdns.org/agent/latest.json"" -UseBasicParsing; ' +
    '$p = Join-Path $env:TEMP (""YummiAgent-Setup-"" + $m.version + "".exe""); ' +
    'if (Test-Path $p) { Remove-Item -LiteralPath $p -Force }; ' +
    '$wc = New-Object System.Net.WebClient; ' +
    '$wc.DownloadFile($m.url, $p); ' +
    '$h = (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant(); ' +
    '$expected = ("" + $m.sha256).ToLowerInvariant(); ' +
    'if ($h -ne $expected) { throw (""SHA-256 mismatch: got "" + $h + "" expected "" + $expected) }; ' +
    'Start-Process -FilePath $p ' +
    '} catch { $_.Exception.Message | Out-File -FilePath $log -Encoding UTF8; exit 1 }' +
    '"';

  if not ShellExec('', 'powershell.exe', Args, '', SW_SHOW, ewWaitUntilTerminated, ResultCode) then
  begin
    MsgBox('최신 설치 파일을 다운로드할 수 없습니다.' + #13#10 +
      '브라우저에서 직접 받기: https://yummi.duckdns.org/agent/latest', mbError, MB_OK);
    Result := False;
    Exit;
  end;

  if ResultCode <> 0 then
  begin
    MsgBox('최신 설치 파일 다운로드 또는 검증에 실패했습니다.' + #13#10 +
      '브라우저: https://yummi.duckdns.org/agent/latest' + #13#10 +
      '로그: %TEMP%\yummi-bootstrap.log', mbError, MB_OK);
    Result := False;
    Exit;
  end;

  Result := False;
end;
