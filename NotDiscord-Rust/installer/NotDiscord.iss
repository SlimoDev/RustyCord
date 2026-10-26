; Standard edition only. Payload lists are generated from an allowlisted local
; build stage by build-installer.ps1. Never compile user data into this file.
#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef NumericVersion
  #define NumericVersion AppVersion + ".0"
#endif
#ifndef PackageFiles
  #error PackageFiles is required
#endif
#ifndef SourceRoot
  #error SourceRoot is required
#endif

[Setup]
AppId=NotDiscord.Standard
AppName=NotDiscord
AppVersion={#AppVersion}
AppVerName=NotDiscord {#AppVersion}
AppPublisher=SlimoDev
AppPublisherURL=https://github.com/SlimoDev/RustyCord
AppSupportURL=https://github.com/SlimoDev/RustyCord/issues
AppUpdatesURL=https://github.com/SlimoDev/RustyCord/releases
DefaultDirName={autopf}\NotDiscord
DefaultGroupName=NotDiscord
AppendDefaultDirName=yes
UsePreviousAppDir=yes
UsePreviousPrivileges=yes
DisableDirPage=no
DisableProgramGroupPage=yes
AllowRootDirectory=no
AllowUNCPath=no
AllowNetworkDrive=no
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog commandline
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
SetupMutex=NotDiscord.Standard.Setup
CloseApplications=no
RestartApplications=no
Uninstallable=yes
UninstallDisplayName=NotDiscord
UninstallDisplayIcon={app}\NotDiscord.exe
UninstallFilesDir={app}\uninstall
OutputBaseFilename=NotDiscord-Setup-{#AppVersion}
SetupIconFile={#SourceRoot}\assets\discord.ico
VersionInfoVersion={#NumericVersion}
VersionInfoDescription=Instalador de NotDiscord
Compression=lzma2/normal
SolidCompression=yes
WizardStyle=modern
DisableWelcomePage=no
AllowCancelDuringInstall=yes
RestartIfNeededByRun=no

[Languages]
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
#include PackageFiles

[Icons]
Name: "{autoprograms}\NotDiscord"; Filename: "{app}\NotDiscord.exe"; Parameters: "{code:ClientParameters}"; WorkingDir: "{app}"; IconFilename: "{app}\discord.ico"; AppUserModelID: "com.notdiscord.Terminal.Native"
Name: "{autodesktop}\NotDiscord"; Filename: "{app}\NotDiscord.exe"; Parameters: "{code:ClientParameters}"; WorkingDir: "{app}"; IconFilename: "{app}\discord.ico"; AppUserModelID: "com.notdiscord.Terminal.Native"

[INI]
Filename: "{app}\installation.ini"; Section: "NotDiscord"; Key: "Variant"; String: "standard"; Flags: uninsdeleteentry uninsdeletesectionifempty
Filename: "{app}\installation.ini"; Section: "NotDiscord"; Key: "Version"; String: "{#AppVersion}"; Flags: uninsdeleteentry uninsdeletesectionifempty
Filename: "{app}\installation.ini"; Section: "NotDiscord"; Key: "Directory"; String: "{app}"; Flags: uninsdeleteentry uninsdeletesectionifempty

[Run]
Filename: "{app}\NotDiscord.exe"; Parameters: "{code:ClientParameters}"; WorkingDir: "{app}"; Description: "Abrir NotDiscord"; Flags: nowait postinstall skipifsilent unchecked; Check: IsInteractiveInstall

[Code]
const
  WAIT_OBJECT_0 = 0;
  WAIT_ABANDONED = $80;
  WAIT_TIMEOUT = 258;
  SYNCHRONIZE = $00100000;
  PROCESS_QUERY_LIMITED_INFORMATION = $1000;
  MUTEX_MODIFY_STATE = $0001;

function WinCreateMutex(Attributes: LongWord; InitialOwner: Integer; Name: String): THandle;
  external 'CreateMutexW@kernel32.dll stdcall';
function WinOpenMutex(Access: Cardinal; Inherit: Integer; Name: String): THandle;
  external 'OpenMutexW@kernel32.dll stdcall';
function WinReleaseMutex(Handle: THandle): Integer;
  external 'ReleaseMutex@kernel32.dll stdcall';
function WinWait(Handle: THandle; Milliseconds: Cardinal): Cardinal;
  external 'WaitForSingleObject@kernel32.dll stdcall';
function WinClose(Handle: THandle): Integer;
  external 'CloseHandle@kernel32.dll stdcall';
function WinCreateEvent(Attributes: LongWord; ManualReset, InitialState: Integer; Name: String): THandle;
  external 'CreateEventW@kernel32.dll stdcall';
function WinSetEvent(Handle: THandle): Integer;
  external 'SetEvent@kernel32.dll stdcall';
function WinOpenProcess(Access: Cardinal; Inherit: Integer; ProcessId: Cardinal): THandle;
  external 'OpenProcess@kernel32.dll stdcall';
function WinProcessPath(Process: THandle; Flags: Cardinal; Buffer: String; var Size: Cardinal): Integer;
  external 'QueryFullProcessImageNameW@kernel32.dll stdcall';
function WinAttributes(Name: String): Cardinal;
  external 'GetFileAttributesW@kernel32.dll stdcall';

var
  Admission, Owner, HelperProcess: THandle;
  AdmissionHeld, OwnerHeld, UpdaterMode, Prepared: Boolean;
  LegacyDataDir, LeaseFile: String;

function Namespace: String;
begin
  Result := 'Local\NotDiscord.Shared.' + Copy(Lowercase(GetSHA256OfString(Utf8Encode(Lowercase(GetEnv('LOCALAPPDATA'))))), 1, 24);
end;

function NormalPath(const Value: String): String;
begin
  Result := RemoveBackslashUnlessRoot(ExpandFileName(Value));
end;

function LocalDirectory(const Value: String): Boolean;
begin
  Result := False;
  if Length(Value) < 3 then Exit;
  Result := (Value[2] = ':') and (Value[3] = '\');
end;

function NoLinks(const Value: String): Boolean;
var P, Parent: String; Attributes: Cardinal;
begin
  Result := False;
  P := NormalPath(Value);
  while P <> '' do begin
    Attributes := WinAttributes(P);
    if (Attributes <> $FFFFFFFF) and ((Attributes and $400) <> 0) then Exit;
    Parent := ExtractFileDir(P);
    if Parent = P then Break;
    P := Parent;
  end;
  Result := True;
end;

function TakeMutex(Handle: THandle; Seconds: Integer): Boolean;
var I: Integer; State: Cardinal;
begin
  Result := False;
  if Handle = 0 then Exit;
  for I := 0 to Seconds * 10 do begin
    State := WinWait(Handle, 100);
    if (State = WAIT_OBJECT_0) or (State = WAIT_ABANDONED) then begin Result := True; Exit; end;
    if State <> WAIT_TIMEOUT then Exit;
  end;
end;

procedure ReleaseLocks;
begin
  if OwnerHeld then begin WinReleaseMutex(Owner); OwnerHeld := False; end;
  if AdmissionHeld then begin WinReleaseMutex(Admission); AdmissionHeld := False; end;
  if Owner <> 0 then begin WinClose(Owner); Owner := 0; end;
  if Admission <> 0 then begin WinClose(Admission); Admission := 0; end;
  if HelperProcess <> 0 then begin WinClose(HelperProcess); HelperProcess := 0; end;
end;

function IsHexId(const Value: String): Boolean;
var I: Integer;
begin
  Result := False;
  if Length(Value) <> 32 then Exit;
  for I := 1 to Length(Value) do
    if Pos(Value[I], '0123456789abcdef') = 0 then Exit;
  Result := True;
end;

function ValidateUpdaterLease: String;
var Work, Updates, InstallDir, HelperImage: String; Size: Integer; Count: Cardinal; Pid: Integer; State: Cardinal;
begin
  Result := 'La autorización del instalador de actualización no es válida. No se modificó la aplicación.';
  LeaseFile := NormalPath(ExpandConstant('{param:NDLEASE|}'));
  Work := ExtractFileDir(LeaseFile);
  Updates := NormalPath(ExpandConstant('{localappdata}\NotDiscord\updates'));
  if (CompareText(ExtractFileDir(Work), Updates) <> 0) or not IsHexId(ExtractFileName(Work))
    or (CompareText(ExtractFileName(LeaseFile), 'installer-lease.ini') <> 0) or not NoLinks(LeaseFile)
    or not FileSize(LeaseFile, Size) or (Size <= 0) or (Size > 65536) then Exit;
  if (GetIniString('NotDiscordUpdater', 'Schema', '', LeaseFile) <> '1')
    or (GetIniString('NotDiscordUpdater', 'Variant', '', LeaseFile) <> 'standard') then Exit;
  InstallDir := NormalPath(GetIniString('NotDiscordUpdater', 'InstallDir', '', LeaseFile));
  if (CompareText(InstallDir, NormalPath(WizardDirValue)) <> 0) or not NoLinks(InstallDir) then Exit;
  if (GetIniString('NotDiscordUpdater', 'AdmissionMutex', '', LeaseFile) <> Namespace + '.Admission')
    or (GetIniString('NotDiscordUpdater', 'OwnerMutex', '', LeaseFile) <> Namespace + '.Owner') then Exit;
  Pid := GetIniInt('NotDiscordUpdater', 'HelperPid', 0, 1, 2147483647, LeaseFile);
  if Pid <= 0 then Exit;
  HelperProcess := WinOpenProcess(PROCESS_QUERY_LIMITED_INFORMATION or SYNCHRONIZE, 0, Pid);
  if HelperProcess = 0 then Exit;
  Count := 32768; SetLength(HelperImage, Count);
  if WinProcessPath(HelperProcess, 0, HelperImage, Count) = 0 then Exit;
  SetLength(HelperImage, Count);
  if (CompareText(HelperImage, ExpandConstant('{win}\System32\WindowsPowerShell\v1.0\powershell.exe')) <> 0)
    and (CompareText(HelperImage, ExpandConstant('{win}\SysWOW64\WindowsPowerShell\v1.0\powershell.exe')) <> 0) then Exit;
  if WinWait(HelperProcess, 0) <> WAIT_TIMEOUT then Exit;
  Admission := WinOpenMutex(SYNCHRONIZE or MUTEX_MODIFY_STATE, 0, Namespace + '.Admission');
  if Admission = 0 then Exit;
  State := WinWait(Admission, 0);
  if (State = WAIT_OBJECT_0) or (State = WAIT_ABANDONED) then begin WinReleaseMutex(Admission); Exit; end;
  if State <> WAIT_TIMEOUT then Exit;
  // The helper keeps Admission; ownership is transferred to this process so a
  // helper crash cannot release the only lock during file replacement/rollback.
  Owner := WinCreateMutex(0, 0, Namespace + '.Owner');
  OwnerHeld := TakeMutex(Owner, 30);
  if not OwnerHeld then Exit;
  Result := '';
end;

function AcquireManualLocks: String;
var Event: THandle;
begin
  Result := 'Otra instancia no terminó su cierre. Cierra NotDiscord normalmente y vuelve a intentar; tus datos se conservan.';
  Admission := WinCreateMutex(0, 0, Namespace + '.Admission');
  AdmissionHeld := TakeMutex(Admission, 30);
  if not AdmissionHeld then Exit;
  Owner := WinCreateMutex(0, 0, Namespace + '.Owner');
  if TakeMutex(Owner, 0) then begin OwnerHeld := True; Result := ''; Exit; end;
  Event := WinCreateEvent(0, 1, 0, Namespace + '.Close');
  if Event <> 0 then begin WinSetEvent(Event); WinClose(Event); end;
  OwnerHeld := TakeMutex(Owner, 120);
  if OwnerHeld then Result := '';
end;

function DataDirArgument(const Arguments: String): String;
var At, Last: Integer; Tail: String;
begin
  Result := '';
  At := Pos('--data-dir ', Lowercase(Arguments));
  if At = 0 then Exit;
  if At > 1 then if Arguments[At - 1] > ' ' then Exit;
  Tail := Trim(Copy(Arguments, At + Length('--data-dir '), MaxInt));
  if Tail = '' then Exit;
  if Tail[1] = '"' then begin
    Delete(Tail, 1, 1); Last := Pos('"', Tail); if Last = 0 then Exit;
    Tail := Copy(Tail, 1, Last - 1);
  end else begin Last := Pos(' ', Tail); if Last > 0 then Tail := Copy(Tail, 1, Last - 1); end;
  if Length(Tail) < 3 then Exit;
  if (Tail[2] <> ':') or (Tail[3] <> '\') or (Pos('"', Tail) > 0)
    or (Pos(#13, Tail) > 0) or (Pos(#10, Tail) > 0) then Exit;
  Result := NormalPath(Tail);
end;

function StandardShortcutTarget(const Link: String; var Arguments: String): String;
var Shell, Shortcut: Variant; Target, CompactVersion: String; VersionText: AnsiString;
begin
  Result := ''; Arguments := '';
  if not FileExists(Link) then Exit;
  try
    Shell := CreateOleObject('WScript.Shell'); Shortcut := Shell.CreateShortcut(Link);
    Target := NormalPath(Shortcut.TargetPath);
    if (CompareText(ExtractFileName(Target), 'NotDiscord.exe') <> 0)
      or (Pos('notdiscord-plus', Lowercase(Target)) > 0) or (Pos('notdiscord+', Lowercase(Target)) > 0) then Exit;
    if FileExists(AddBackslash(ExtractFileDir(Target)) + 'version.json') then begin
      if not LoadStringFromFile(AddBackslash(ExtractFileDir(Target)) + 'version.json', VersionText) then Exit;
      CompactVersion := Lowercase(String(VersionText));
      StringChangeEx(CompactVersion, ' ', '', True); StringChangeEx(CompactVersion, #9, '', True);
      StringChangeEx(CompactVersion, #13, '', True); StringChangeEx(CompactVersion, #10, '', True);
      if Pos('"variant":"plus"', CompactVersion) > 0 then Exit;
    end;
    Arguments := Shortcut.Arguments; Result := Target;
  except Result := ''; end;
end;

procedure CaptureLegacyData;
var Target, Arguments: String;
begin
  LegacyDataDir := '';
  // Never place a specific user's portable path into shared all-user links.
  if IsAdminInstallMode then Exit;
  Target := StandardShortcutTarget(ExpandConstant('{userdesktop}\NotDiscord Rust.lnk'), Arguments);
  if Target = '' then Target := StandardShortcutTarget(ExpandConstant('{userdesktop}\NotDiscord.lnk'), Arguments);
  if Target = '' then Target := StandardShortcutTarget(ExpandConstant('{userprograms}\NotDiscord.lnk'), Arguments);
  if Target <> '' then LegacyDataDir := DataDirArgument(Arguments);
end;

function ClientParameters(Param: String): String;
begin
  Result := '--runtime-root "' + ExpandConstant('{app}') + '"';
  if LegacyDataDir <> '' then Result := Result + ' --data-dir "' + LegacyDataDir + '"';
end;

function IsInteractiveInstall: Boolean;
begin Result := not UpdaterMode; end;

function InitializeSetup: Boolean;
begin
  UpdaterMode := ExpandConstant('{param:NOTDISCORDUPDATER|0}') = '1';
  Result := True;
  if UpdaterMode and (ExpandConstant('{param:NDLEASE|}') = '') then begin
    MsgBox('Falta la autorización de la actualización. Usa el instalador normalmente o inicia Actualizar desde NotDiscord.', mbError, MB_OK);
    Result := False;
  end;
end;

procedure InitializeWizard;
begin
  CaptureLegacyData;
  WizardForm.SelectDirLabel.Caption := 'Elige la carpeta de instalación. Si indicas una carpeta base, se creará una subcarpeta NotDiscord. La ruta anterior se conserva al actualizar.';
end;

function NextButtonClick(CurPageID: Integer): Boolean;
var Dir: String;
begin
  Result := True;
  if (CurPageID = wpSelectDir) and not UpdaterMode then begin
    if not LocalDirectory(Trim(WizardDirValue)) then begin
      MsgBox('Elige una ruta local completa, por ejemplo D:\Aplicaciones. Se añadirá la subcarpeta NotDiscord.', mbError, MB_OK);
      Result := False; Exit;
    end;
    Dir := NormalPath(WizardDirValue);
    if CompareText(ExtractFileName(Dir), 'NotDiscord') <> 0 then Dir := AddBackslash(Dir) + 'NotDiscord';
    WizardForm.DirEdit.Text := Dir;
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if Prepared then Exit;
  if not LocalDirectory(WizardDirValue) then begin Result := 'Elige una ruta local completa para instalar NotDiscord.'; Exit; end;
  if not NoLinks(WizardDirValue) then begin Result := 'Elige una carpeta local sin enlaces ni redirecciones.'; Exit; end;
  if UpdaterMode then Result := ValidateUpdaterLease else Result := AcquireManualLocks;
  if Result <> '' then begin ReleaseLocks; Exit; end;
  Prepared := True;
end;

procedure CheckInstallerLease;
begin
  if UpdaterMode and ((HelperProcess = 0) or (WinWait(HelperProcess, 0) <> WAIT_TIMEOUT)) then
    RaiseException('El coordinador de actualización se cerró. Se cancela la instalación para conservar la versión anterior.');
end;

procedure CurStepChanged(CurStep: TSetupStep);
var Arguments, Target, Link: String;
begin
  if CurStep = ssPostInstall then begin
    // Retire only the known standard legacy link, after successful copying.
    // Never inspect/delete or rewrite NotDiscord+.lnk.
    Link := ExpandConstant('{userdesktop}\NotDiscord Rust.lnk');
    Target := StandardShortcutTarget(Link, Arguments);
    if Target <> '' then DeleteFile(Link);
  end;
end;

procedure DeinitializeSetup;
begin ReleaseLocks; end;

function InitializeUninstall: Boolean;
var Error: String;
begin
  Error := AcquireManualLocks; Result := Error = '';
  if not Result then begin ReleaseLocks; MsgBox(Error, mbError, MB_OK); end;
end;

procedure DeinitializeUninstall;
begin ReleaseLocks; end;
