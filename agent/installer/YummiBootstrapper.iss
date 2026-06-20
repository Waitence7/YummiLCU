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
function BootstrapLogPath: String;
begin
  Result := ExpandConstant('{localappdata}\Yummi\bootstrap.log');
end;

procedure AppendBootstrapLog(const Line: String);
var
  Dir: String;
begin
  Dir := ExtractFileDir(BootstrapLogPath);
  if not DirExists(Dir) then
    ForceDirectories(Dir);
  if not SaveStringToFile(BootstrapLogPath, Line + #13#10, True) then
  begin
    { localappdata 실패 시 tmp 폴백 }
    SaveStringToFile(ExpandConstant('{tmp}\yummi-bootstrap.log'), Line + #13#10, True);
  end;
end;

function WriteBootstrapScript(const ScriptPath: String): Boolean;
var
  Lines: TArrayOfString;
begin
  SetArrayLength(Lines, 31);
  Lines[0] := '$ErrorActionPreference = ''Stop''';
  Lines[1] := '$ProgressPreference = ''SilentlyContinue''';
  Lines[2] := '$logDir = Join-Path $env:LOCALAPPDATA ''Yummi''';
  Lines[3] := 'New-Item -ItemType Directory -Force -Path $logDir | Out-Null';
  Lines[4] := '$log = Join-Path $logDir ''bootstrap.log''';
  Lines[5] := 'function Log([string]$m) { "$(Get-Date -Format o) $m" | Add-Content -LiteralPath $log }';
  Lines[6] := '$manifestUri = [Uri]''https://yummi.duckdns.org/agent/latest.json''';
  Lines[7] := '$allowedHost = ''yummi.duckdns.org''';
  Lines[8] := 'try {';
  Lines[9] := '  Log ''bootstrap start''';
  Lines[10] := '  [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12';
  Lines[11] := '  $m = Invoke-RestMethod -Uri $manifestUri.AbsoluteUri -UseBasicParsing';
  Lines[12] := '  if (-not $m.version -or -not $m.url -or -not $m.sha256) { throw ''manifest missing required fields'' }';
  Lines[13] := '  $downloadUri = [Uri]$m.url';
  Lines[14] := '  if ($downloadUri.Scheme -ne ''https'') { throw (''installer URL must use https: '' + $downloadUri.AbsoluteUri) }';
  Lines[15] := '  if ($downloadUri.Host -ne $allowedHost) { throw (''installer host not allowed: '' + $downloadUri.Host) }';
  Lines[16] := '  if (-not ($m.sha256 -match ''^[A-Fa-f0-9]{64}$'')) { throw ''manifest sha256 must be 64 hex chars'' }';
  Lines[17] := '  Log (''manifest version='' + $m.version)';
  Lines[18] := '  $p = Join-Path $env:TEMP (''YummiAgent-Setup-'' + $m.version + ''.exe'')';
  Lines[19] := '  if (Test-Path -LiteralPath $p) { Remove-Item -LiteralPath $p -Force }';
  Lines[20] := '  $wc = New-Object System.Net.WebClient';
  Lines[21] := '  Log (''download '' + $downloadUri.AbsoluteUri + '' -> '' + $p)';
  Lines[22] := '  $wc.DownloadFile($downloadUri.AbsoluteUri, $p)';
  Lines[23] := '  $h = (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant()';
  Lines[24] := '  $expected = ('''' + $m.sha256).ToLowerInvariant()';
  Lines[25] := '  if ($h -ne $expected) { throw (''SHA-256 mismatch: got '' + $h + '' expected '' + $expected) }';
  Lines[26] := '  Log ''hash ok; launching installer''';
  Lines[27] := '  Start-Process -FilePath $p';
  Lines[28] := '} catch { Log (''ERROR: '' + ($_ | Out-String)); exit 1 }';
  Lines[29] := '';
  Lines[30] := '';
  Result := SaveStringsToFile(ScriptPath, Lines, False);
end;

function InitializeSetup(): Boolean;
var
  ResultCode: Integer;
  ScriptPath: String;
  Args: String;
  LogHint: String;
begin
  LogHint := BootstrapLogPath;
  ScriptPath := ExpandConstant('{tmp}\yummi-bootstrap.ps1');
  AppendBootstrapLog('Inno bootstrapper start');

  if not WriteBootstrapScript(ScriptPath) then
  begin
    AppendBootstrapLog('failed to write bootstrap script: ' + ScriptPath);
    MsgBox('부트스트래퍼 스크립트를 만들 수 없습니다.' + #13#10 +
      '로그: ' + LogHint, mbError, MB_OK);
    Result := False;
    Exit;
  end;

  Args := '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "' + ScriptPath + '"';

  if not ShellExec('', 'powershell.exe', Args, '', SW_SHOW, ewWaitUntilTerminated, ResultCode) then
  begin
    AppendBootstrapLog('ShellExec(powershell) failed');
    MsgBox('최신 설치 파일을 다운로드할 수 없습니다.' + #13#10 +
      '브라우저: https://yummi.duckdns.org/agent/latest' + #13#10 +
      '로그: ' + LogHint, mbError, MB_OK);
    Result := False;
    Exit;
  end;

  if ResultCode <> 0 then
  begin
    AppendBootstrapLog('powershell exit code=' + IntToStr(ResultCode));
    MsgBox('최신 설치 파일 다운로드 또는 검증에 실패했습니다.' + #13#10 +
      '브라우저: https://yummi.duckdns.org/agent/latest' + #13#10 +
      '로그: ' + LogHint, mbError, MB_OK);
    Result := False;
    Exit;
  end;

  AppendBootstrapLog('bootstrap finished ok');
  Result := False;
end;
