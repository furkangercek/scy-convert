; Per-user installer for scyconvert. Built by package.ps1, which passes
; AppVersion, Payload (the staged files) and OutputDir.

#define AppName "scyconvert"
; Must match CLSID in crates/scyconvert-shell/src/windows.rs.
#define ShellClsid "{{BB1183D6-E6CA-44E1-906C-A0A47845841D}"

[Setup]
AppId={{8CDBA0D1-494B-4C1E-9201-43327E889181}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=furkangercek
AppPublisherURL=https://github.com/furkangercek/scy-convert
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesEnvironment=yes
ChangesAssociations=yes
LicenseFile={#Payload}\LICENSE.txt
OutputDir={#OutputDir}
OutputBaseFilename={#AppName}-{#AppVersion}-windows-x64-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\scyconvert-app.exe
#if FileExists(Payload + "\scyconvert.ico")
SetupIconFile={#Payload}\scyconvert.ico
#endif

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked
Name: "addtopath"; Description: "Add the scyconvert command to PATH"
Name: "explorermenu"; Description: "Add ""Convert with scyconvert"" to the Explorer right-click menu"
Name: "startup"; Description: "Start scyconvert minimized when I sign in"

[Files]
Source: "{#Payload}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\scyconvert-app.exe"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\scyconvert-app.exe"; Tasks: desktopicon

[Registry]
; The Explorer menu: a per-user COM server for the IExplorerCommand handler in
; scyconvert_shell.dll, and a verb on every file type that uses it. On Windows
; 11 it appears under "Show more options".
Root: HKCU; Subkey: "Software\Classes\CLSID\{#ShellClsid}"; Flags: uninsdeletekey; Tasks: explorermenu
Root: HKCU; Subkey: "Software\Classes\CLSID\{#ShellClsid}\InprocServer32"; ValueType: string; \
  ValueData: "{app}\scyconvert_shell.dll"; Tasks: explorermenu
Root: HKCU; Subkey: "Software\Classes\CLSID\{#ShellClsid}\InprocServer32"; ValueType: string; \
  ValueName: "ThreadingModel"; ValueData: "Apartment"; Tasks: explorermenu
Root: HKCU; Subkey: "Software\Classes\*\shell\scyconvert"; Flags: uninsdeletekey; Tasks: explorermenu
Root: HKCU; Subkey: "Software\Classes\*\shell\scyconvert"; ValueType: string; \
  ValueName: "MUIVerb"; ValueData: "Convert with scyconvert"; Tasks: explorermenu
Root: HKCU; Subkey: "Software\Classes\*\shell\scyconvert"; ValueType: string; \
  ValueName: "ExplorerCommandHandler"; ValueData: "{#ShellClsid}"; Tasks: explorermenu
; Same value as "Open at login" in Settings (crates/scyconvert-app/src/login.rs).
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; \
  ValueName: "scyconvert"; ValueData: """{app}\scyconvert-app.exe"" --minimized"; \
  Flags: uninsdeletevalue; Tasks: startup
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; \
  ValueData: "{olddata};{app}"; Tasks: addtopath; Check: NeedsAddPath(ExpandConstant('{app}'))

[Run]
Filename: "{app}\scyconvert-app.exe"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent

[Code]
const
  ShellDll = 'scyconvert_shell.dll';

{ Explorer keeps the menu DLL loaded, so it can't be overwritten or deleted.
  A loaded DLL can still be renamed: move it aside, then remove old copies
  once Explorer has let go of them. }
procedure MoveShellDllAside;
var
  Dll: string;
begin
  Dll := ExpandConstant('{app}\') + ShellDll;
  if FileExists(Dll) then
    RenameFile(Dll, Dll + '.' + GetDateTimeString('yyyymmddhhnnss', #0, #0) + '.old');
end;

procedure DeleteOldShellDlls;
var
  Found: TFindRec;
  Dir: string;
begin
  Dir := ExpandConstant('{app}\');
  if FindFirst(Dir + ShellDll + '.*.old', Found) then
  try
    repeat
      DeleteFile(Dir + Found.Name);
    until not FindNext(Found);
  finally
    FindClose(Found);
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then
  begin
    DeleteOldShellDlls;
    MoveShellDllAside;
  end;
end;

function NeedsAddPath(Dir: string): Boolean;
var
  Path: string;
begin
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', Path) then
  begin
    Result := True;
    exit;
  end;
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Path) + ';') = 0;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Path, Dir: string;
  P: Integer;
begin
  if CurUninstallStep = usUninstall then
    MoveShellDllAside;
  if CurUninstallStep <> usPostUninstall then
    exit;
  DeleteOldShellDlls;
  RemoveDir(ExpandConstant('{app}'));
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', Path) then
    exit;
  Dir := ExpandConstant('{app}');
  P := Pos(';' + Uppercase(Dir), Uppercase(Path));
  if P > 0 then
  begin
    Delete(Path, P, Length(Dir) + 1);
    RegWriteExpandStringValue(HKCU, 'Environment', 'Path', Path);
  end;
end;
