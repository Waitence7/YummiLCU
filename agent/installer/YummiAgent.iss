; Yummi LCU Agent — Inno Setup
; 빌드: build-installer.bat  또는  ISCC.exe /DAppVersion=0.3.1 YummiAgent.iss

#ifndef AppVersion
  #define AppVersion "0.3.1"
#endif

#define AppName "Yummi Agent"
#define AppExe "YummiLcu.App.exe"
#define PublishDir "..\YummiLcu.App\bin\Release\net8.0-windows\win-x64\publish"
#define OutputDir "output"

[Setup]
AppId={{A7B3C9E1-4F2D-4A8B-9C6E-1D2E3F4A5B6C}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Yummi
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
OutputDir={#OutputDir}
OutputBaseFilename=YummiAgent-Setup-{#AppVersion}
SetupIconFile=
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\{#AppExe}
UsePreviousAppDir=yes
CloseApplications=force

[Languages]
Name: "korean"; MessagesFile: "compiler:Languages\Korean.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#PublishDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\agent.json.example"; DestDir: "{app}"; DestName: "agent.json"; Flags: onlyifdoesntexist uninsneveruninstall

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#StringChange(AppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[Code]
function IsDotNetDesktop8Installed: Boolean;
var
  TempFile: String;
  Lines: TArrayOfString;
  i: Integer;
  ResultCode: Integer;
begin
  TempFile := ExpandConstant('{tmp}\yummi-dotnet-runtimes.txt');
  if Exec(ExpandConstant('{cmd}'), '/c dotnet --list-runtimes > "' + TempFile + '" 2>&1', '', SW_HIDE, ewWaitUntilTerminated, ResultCode) then
  begin
    if LoadStringsFromFile(TempFile, Lines) then
    begin
      for i := 0 to GetArrayLength(Lines) - 1 do
        if Pos('Microsoft.WindowsDesktop.App 8.', Lines[i]) > 0 then
        begin
          Result := True;
          exit;
        end;
    end;
  end;
  Result := False;
end;

function InitializeSetup: Boolean;
var
  ResultCode: Integer;
begin
  if not IsDotNetDesktop8Installed then
  begin
    if MsgBox('.NET 8 Desktop Runtime 이 필요합니다.' + #13#10 +
      'https://dotnet.microsoft.com/download/dotnet/8.0 에서 설치 후 다시 실행해 주세요.' + #13#10#13#10 +
      '설치 페이지를 열까요?', mbConfirmation, MB_YESNO) = IDYES then
      ShellExec('open', 'https://dotnet.microsoft.com/download/dotnet/8.0', '', '', SW_SHOWNORMAL, ewNoWait, ResultCode);
    Result := False;
    exit;
  end;
  Result := True;
end;
