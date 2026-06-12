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
    '$m = Invoke-RestMethod -Uri ""https://yummi.duckdns.org/agent/latest.json""; ' +
    '$p = Join-Path $env:TEMP (""YummiAgent-Setup-"" + $m.version + "".exe""); ' +
    'Invoke-WebRequest -Uri $m.url -OutFile $p; ' +
    '$h = (Get-FileHash -Algorithm SHA256 $p).Hash.ToLowerInvariant(); ' +
    'if ($h -ne $m.sha256.ToLowerInvariant()) { throw ""SHA-256 mismatch"" }; ' +
    'Start-Process -FilePath $p' +
    '"';

  if not ShellExec('', 'powershell.exe', Args, '', SW_SHOW, ewWaitUntilTerminated, ResultCode) then
  begin
    MsgBox('최신 설치 파일을 다운로드할 수 없습니다.', mbError, MB_OK);
    Result := False;
    Exit;
  end;

  if ResultCode <> 0 then
  begin
    MsgBox('최신 설치 파일 다운로드 또는 검증에 실패했습니다.', mbError, MB_OK);
    Result := False;
    Exit;
  end;

  Result := False;
end;
