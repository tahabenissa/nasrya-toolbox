; Winsploit installer (Inno Setup 6). Per-user, no UAC.
;
; Build (from the repo root, after `cargo build --release`):
;   target\release\winsploit.exe packs --iss > installer\packs.iss
;   ISCC /DAppVersion=0.2.0 installer\winsploit.iss
; or run installer\build.ps1. The pack list comes from tools.yaml, so the
; checkboxes can't drift from the catalog.
;
; Silent install:
;   winsploit-<ver>-setup.exe /VERYSILENT /SUPPRESSMSGBOXES ^
;     /COMPONENTS="main,path,scoop,buckets" /TASKS="wtprofile" /PACKS=recon,web
; (/PACKS is ours: a comma-separated list of pack ids, see `winsploit packs`.)

#ifndef AppVersion
  #define AppVersion "0.0.0-dev"
#endif
#include "packs.iss"
#define AppGuid "{8E3B6C52-4B1A-4F5D-9C7E-2D0A5F61B7A4}"

[Setup]
; Fixed AppId: a newer setup upgrades in place and remembers earlier choices.
AppId={{8E3B6C52-4B1A-4F5D-9C7E-2D0A5F61B7A4}
AppName=Winsploit
AppVersion={#AppVersion}
AppPublisher=Winsploit
AppPublisherURL=https://github.com/tahabenissa/winsploit
AppSupportURL=https://github.com/tahabenissa/winsploit/issues
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\Winsploit
DisableProgramGroupPage=yes
DisableDirPage=auto
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
MinVersion=10.0
WizardStyle=modern
LicenseFile=notice.txt
OutputDir=..\dist
OutputBaseFilename=winsploit-{#AppVersion}-setup
Compression=lzma2
SolidCompression=yes
; user PATH changes must reach new terminals
ChangesEnvironment=yes
; a running winsploit.exe locks the exe: ask the user to close it
CloseApplications=yes
CloseApplicationsFilter=winsploit.exe
RestartApplications=no
UninstallDisplayName=Winsploit
UsePreviousTasks=yes

[Messages]
FinishedLabel=Winsploit is installed.%n%nIf you ticked tool packs, Winsploit lists every tool and asks before it installs anything.%n%nGood to know:%n- The exe is unsigned. If SmartScreen or Smart App Control blocks it, choose "More info", then "Run anyway" (or allow it in Smart App Control).%n- Defender may quarantine offensive tools. Setup never adds exclusions.%n- Packet tools such as nmap want Npcap: get it from npcap.com (its license forbids bundling it).%n- tools.yaml next to the exe is replaced on every upgrade. Put your own catalog in the winsploit folder of your AppData\Roaming, or pass --catalog.
ConfirmUninstall=Remove %1?%n%nScoop, the tools you installed through it and your ctf folder are never touched.

[Types]
Name: "full"; Description: "Recommended"
Name: "custom"; Description: "Custom"; Flags: iscustom

[Components]
Name: "main"; Description: "Winsploit (winsploit.exe, tools.yaml, README, LICENSE)"; Types: full custom; Flags: fixed
Name: "path"; Description: "Add Winsploit to PATH (new terminals can run `winsploit`)"; Types: full
Name: "scoop"; Description: "Install Scoop + git (only if missing)"; Types: full
Name: "buckets"; Description: "Add the catalog's Scoop buckets"; Types: full
Name: "pipx"; Description: "Python + pipx (for the pipx-based tools)"
Name: "startmenu"; Description: "Start menu shortcut"; Types: full
Name: "desktop"; Description: "Desktop shortcut"; Types: full

[Tasks]
Name: "wtprofile"; Description: "Add a Windows Terminal profile"; Check: WtInstalled

[Files]
Source: "..\target\release\winsploit.exe"; DestDir: "{app}"; Flags: ignoreversion; Components: main
; the shipped catalog: replaced on upgrade (personal edits go in %APPDATA%\winsploit\tools.yaml)
Source: "..\tools.yaml"; DestDir: "{app}"; Flags: ignoreversion; Components: main
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion; Components: main
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion; Components: main

[Icons]
Name: "{autoprograms}\Winsploit"; Filename: "{app}\winsploit.exe"; Components: startmenu
Name: "{autodesktop}\Winsploit"; Filename: "{app}\winsploit.exe"; Components: desktop

[UninstallDelete]
Type: filesandordirs; Name: "{localappdata}\Microsoft\Windows Terminal\Fragments\Winsploit"

[Run]
; Clicking Finish starts the dashboard, in Windows Terminal when it is installed,
; otherwise in a new console window. Ticked by default.
Filename: "{localappdata}\Microsoft\WindowsApps\wt.exe"; Parameters: "new-tab --title Winsploit ""{app}\winsploit.exe"""; Description: "Launch Winsploit now"; Flags: postinstall nowait skipifsilent runasoriginaluser; Check: WtInstalled
Filename: "{app}\winsploit.exe"; Description: "Launch Winsploit now"; Flags: postinstall nowait skipifsilent runasoriginaluser; Check: not WtInstalled

[Code]
const
  PackListData = '{#PackList}';
  { indexes in [Components] }
  CompScoop = 2;
  CompBuckets = 3;
  CompPipx = 4;

var
  PackPage: TInputOptionWizardPage;
  PackIds: array of String;
  HasScoop, HasGit, HasPipx: Boolean;
  LogFile: String;
  DeleteUserData: Boolean;

function SetEnvironmentVariableW(Name, Value: String): Boolean;
  external 'SetEnvironmentVariableW@kernel32.dll stdcall';

{ ---- helpers ---------------------------------------------------------- }

procedure Note(const Msg: String);
begin
  if LogFile <> '' then
    SaveStringToFile(LogFile,
      GetDateTimeString('yyyy-mm-dd hh:nn:ss', '-', ':') + '  ' + Msg + #13#10, True);
end;

function OnPath(const Exe: String): Boolean;
begin
  Result := FileSearch(Exe, GetEnv('PATH')) <> '';
end;

function WtInstalled: Boolean;
begin
  Result := FileExists(ExpandConstant('{localappdata}\Microsoft\WindowsApps\wt.exe'));
end;

function NextField(var S: String; const Sep: String): String;
var
  P: Integer;
begin
  P := Pos(Sep, S);
  if P = 0 then
  begin
    Result := S;
    S := '';
  end
  else
  begin
    Result := Copy(S, 1, P - 1);
    S := Copy(S, P + 1, Length(S));
  end;
end;

{ Only [a-z0-9_-] and commas survive: pack ids end up in a YAML file. }
function SafePacks(const S: String): String;
var
  I: Integer;
  C: Char;
begin
  Result := '';
  for I := 1 to Length(S) do
  begin
    C := S[I];
    if (C >= 'A') and (C <= 'Z') then
      C := Chr(Ord(C) + 32);
    if ((C >= 'a') and (C <= 'z')) or ((C >= '0') and (C <= '9')) or
       (C = '-') or (C = '_') or (C = ',') then
      Result := Result + C;
  end;
end;

{ ---- user PATH -------------------------------------------------------- }

function SamePath(const A, B: String): Boolean;
var
  X, Y: String;
begin
  X := Lowercase(A);
  Y := Lowercase(B);
  if (X <> '') and (X[Length(X)] = '\') then X := Copy(X, 1, Length(X) - 1);
  if (Y <> '') and (Y[Length(Y)] = '\') then Y := Copy(Y, 1, Length(Y) - 1);
  Result := X = Y;
end;

procedure AddToPath(const Dir: String);
var
  Cur, Rest, Entry: String;
begin
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', Cur) then Cur := '';
  Rest := Cur;
  while Rest <> '' do
  begin
    Entry := NextField(Rest, ';');
    if SamePath(Entry, Dir) then Exit;
  end;
  if (Cur <> '') and (Cur[Length(Cur)] <> ';') then Cur := Cur + ';';
  RegWriteExpandStringValue(HKCU, 'Environment', 'Path', Cur + Dir);
  Note('added to user PATH: ' + Dir);
end;

{ Removes only our entry and keeps everything else as it was. }
procedure RemoveFromPath(const Dir: String);
var
  Cur, Rest, Entry, Kept: String;
begin
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', Cur) then Exit;
  Rest := Cur;
  Kept := '';
  while Rest <> '' do
  begin
    Entry := NextField(Rest, ';');
    if (Entry <> '') and not SamePath(Entry, Dir) then
    begin
      if Kept <> '' then Kept := Kept + ';';
      Kept := Kept + Entry;
    end;
  end;
  if Kept <> Cur then
    RegWriteExpandStringValue(HKCU, 'Environment', 'Path', Kept);
end;

{ ---- environment detection -------------------------------------------- }

procedure DetectEnvironment;
var
  Ver: String;
begin
  HasScoop := OnPath('scoop.cmd') or
    FileExists(ExpandConstant('{%USERPROFILE}\scoop\shims\scoop.cmd'));
  HasGit := OnPath('git.exe');
  HasPipx := OnPath('pipx.exe') or OnPath('pipx.cmd');
  Note('scoop found: ' + IntToStr(Ord(HasScoop)) + ', git: ' + IntToStr(Ord(HasGit)) +
       ', pipx: ' + IntToStr(Ord(HasPipx)) + ', elevated: ' + IntToStr(Ord(IsAdmin)));
  if RegQueryStringValue(HKCU,
       'Software\Microsoft\Windows\CurrentVersion\Uninstall\{#AppGuid}_is1',
       'DisplayVersion', Ver) then
    Note('previous install: ' + Ver);
end;

{ Components that are already satisfied are shown as skipped; Scoop cannot
  be bootstrapped from an elevated setup. Safe to call repeatedly: the page
  restores earlier choices when it is shown. }
procedure ApplyEnvironment;
var
  L: TNewCheckListBox;
begin
  L := WizardForm.ComponentsList;
  if L.Items.Count <= CompPipx then Exit;
  if IsAdmin then
  begin
    L.ItemCaption[CompScoop] := 'Install Scoop + git (unavailable: Scoop refuses to be installed elevated; run setup without "Run as administrator")';
    L.Checked[CompScoop] := False;
    L.ItemEnabled[CompScoop] := False;
  end
  else if HasScoop and HasGit then
  begin
    L.ItemCaption[CompScoop] := 'Install Scoop + git (found, skipping)';
    L.Checked[CompScoop] := False;
    L.ItemEnabled[CompScoop] := False;
  end;
  if HasPipx then
  begin
    L.ItemCaption[CompPipx] := 'Python + pipx (found, skipping)';
    L.Checked[CompPipx] := False;
    L.ItemEnabled[CompPipx] := False;
  end;
end;

{ ---- tool packs ------------------------------------------------------- }

procedure InitPacks;
var
  Rest, Item, Id, Lbl, Cnt, Def, Want, Cap: String;
  HasParam: Boolean;
  I: Integer;
begin
  PackPage := CreateInputOptionPage(wpSelectComponents, 'Tool packs',
    'Which tools should Winsploit install for you?',
    'Each pack is one category of the catalog. Winsploit lists every tool and asks before it installs anything the first time you start it. You can install more later with "i".',
    False, False);
  Want := SafePacks(ExpandConstant('{param:PACKS|}'));
  HasParam := Want <> '';
  Want := ',' + Want + ',';
  Rest := PackListData;
  while Rest <> '' do
  begin
    Item := NextField(Rest, ';');
    Id := NextField(Item, '|');
    Lbl := NextField(Item, '|');
    Cnt := NextField(Item, '|');
    Def := NextField(Item, '|');
    Cap := Lbl + ' (' + Cnt + ')';
    if Id = 'wordlists' then Cap := Cap + '  - large download, off by default';
    I := PackPage.Add(Cap);
    SetArrayLength(PackIds, I + 1);
    PackIds[I] := Id;
    if HasParam then
      PackPage.Values[I] := Pos(',' + Id + ',', Want) > 0
    else
      PackPage.Values[I] := GetPreviousData('Pack_' + Id, Def) = '1';
  end;
end;

procedure RegisterPreviousData(PreviousDataKey: Integer);
var
  I: Integer;
begin
  for I := 0 to GetArrayLength(PackIds) - 1 do
    if PackPage.Values[I] then
      SetPreviousData(PreviousDataKey, 'Pack_' + PackIds[I], '1')
    else
      SetPreviousData(PreviousDataKey, 'Pack_' + PackIds[I], '0');
end;

{ Writes (or removes) the file Winsploit reads on its next start. The file
  only names packs: Winsploit asks before installing anything. }
procedure WriteFirstRun;
var
  Dir, FileName, List: String;
  I: Integer;
begin
  Dir := ExpandConstant('{userappdata}\winsploit');
  FileName := Dir + '\first-run.yaml';
  List := '';
  for I := 0 to GetArrayLength(PackIds) - 1 do
    if PackPage.Values[I] then
    begin
      if List <> '' then List := List + ', ';
      List := List + PackIds[I];
    end;
  if List = '' then
  begin
    DeleteFile(FileName);
    Exit;
  end;
  ForceDirectories(Dir);
  SaveStringToFile(FileName, 'packs: [' + List + ']' + #13#10, False);
  Note('first-run packs: ' + List);
end;

{ ---- Windows Terminal profile ----------------------------------------- }

procedure WriteWtFragment;
var
  Dir, Exe: String;
begin
  Dir := ExpandConstant('{localappdata}\Microsoft\Windows Terminal\Fragments\Winsploit');
  ForceDirectories(Dir);
  Exe := ExpandConstant('{app}\winsploit.exe');
  StringChangeEx(Exe, '\', '\\', True);
  SaveStringToFile(Dir + '\winsploit.json',
    '{"profiles":[{"name":"Winsploit","commandline":"' + Exe +
    '","startingDirectory":"%USERPROFILE%","font":{"face":"Cascadia Mono"}}]}', False);
  Note('Windows Terminal fragment written');
end;

{ ---- bootstrap -------------------------------------------------------- }

function Run(const Exe, Params, What: String): Boolean;
var
  Code: Integer;
begin
  WizardForm.StatusLabel.Caption := What + '...';
  WizardForm.FilenameLabel.Caption := Exe;
  Note('run: ' + What);
  Result := Exec(Exe, Params, '', SW_HIDE, ewWaitUntilTerminated, Code) and (Code = 0);
  if Result then
    Note('  ok')
  else
    Note('  FAILED (exit ' + IntToStr(Code) + ')');
end;

procedure Bootstrap;
var
  Scoop, Params: String;
  Ok: Boolean;
begin
  Scoop := ExpandConstant('{%USERPROFILE}\scoop\shims\scoop.cmd');
  if WizardIsComponentSelected('scoop') and not IsAdmin then
  begin
    if not HasScoop then
    begin
      { Official installer over HTTPS, process-scope bypass only: the user's
        execution policy is not changed. Output goes to the setup log. }
      Note('fetching https://get.scoop.sh (the official Scoop installer)');
      SetEnvironmentVariableW('WINSPLOIT_SETUP_LOG', LogFile);
      Ok := Run(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
        '-NoProfile -ExecutionPolicy Bypass -Command "irm get.scoop.sh | iex *>> $env:WINSPLOIT_SETUP_LOG"',
        'Installing Scoop');
      HasScoop := Ok and FileExists(Scoop);
    end;
    if HasScoop and not HasGit then
      Run(Scoop, 'install git', 'Installing git');
  end;

  Params := '';
  if WizardIsComponentSelected('buckets') then Params := Params + ' --buckets';
  if WizardIsComponentSelected('pipx') then Params := Params + ' --pipx';
  if Params <> '' then
  begin
    if HasScoop or OnPath('scoop.cmd') then
      Run(ExpandConstant('{app}\winsploit.exe'), 'setup' + Params,
        'Setting up buckets / pipx (winsploit setup' + Params + ')')
    else
      Note('buckets / pipx skipped: Scoop is not installed');
  end;
end;

{ ---- wizard events ---------------------------------------------------- }

procedure InitializeWizard;
var
  Dir: String;
begin
  Dir := ExpandConstant('{localappdata}\winsploit\logs');
  ForceDirectories(Dir);
  LogFile := Dir + '\setup-' + GetDateTimeString('yyyymmdd-hhnnss', #0, #0) + '.log';
  Note('winsploit setup {#AppVersion} started');
  DetectEnvironment;
  InitPacks;
  ApplyEnvironment;
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if CurPageID = wpSelectComponents then
    ApplyEnvironment;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    if WizardIsComponentSelected('path') then
      AddToPath(ExpandConstant('{app}'));
    if WizardIsTaskSelected('wtprofile') then
      WriteWtFragment;
    WriteFirstRun;
    Bootstrap;
    Note('setup finished');
  end;
end;

{ ---- uninstall -------------------------------------------------------- }

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    RemoveFromPath(ExpandConstant('{app}'));
    { off by default, and always "no" in silent runs }
    DeleteUserData := UninstallSilent = False;
    if DeleteUserData then
      DeleteUserData := MsgBox('Also delete Winsploit''s settings and logs?' + #13#10 +
        '(%APPDATA%\winsploit and %LOCALAPPDATA%\winsploit\logs)' + #13#10#13#10 +
        'Scoop, installed tools and your ctf folder are never deleted.',
        mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES;
  end
  else if (CurUninstallStep = usPostUninstall) and DeleteUserData then
  begin
    DelTree(ExpandConstant('{userappdata}\winsploit'), True, True, True);
    DelTree(ExpandConstant('{localappdata}\winsploit\logs'), True, True, True);
    RemoveDir(ExpandConstant('{localappdata}\winsploit'));
  end;
end;
