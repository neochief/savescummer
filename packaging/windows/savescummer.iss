; SaveScummer's per-user Windows installer (PLAN-BUILD.md Windows).
;
; Only `cargo xtask dist` compiles this. It passes:
;   AppVersion          the Cargo version
;   Payload             the release APP PACKAGE, installed exactly as it is
;   OutputDir           dist\
;   OutputBaseFilename  SaveScummer-windows-x64-<version>-setup
;   RepoRoot            the repository, for the icon
;   MinWindows          the minimum Windows version
;
; Per-user, so no admin prompt. User data (%LOCALAPPDATA%\SaveScummer and a
; checkpoint store the user moved elsewhere) is never touched: nothing here
; names it.

#ifndef AppVersion
  #error Build the installer with `cargo xtask dist`.
#endif

[Setup]
; Fixed forever: upgrades find the previous install by it.
AppId={{A47AAE37-E015-46C5-9725-87F096AA0DD1}
AppName=SaveScummer
AppVersion={#AppVersion}
AppVerName=SaveScummer {#AppVersion}
AppPublisher=SaveScummer
AppPublisherURL=https://github.com/neochief/savescummer
AppSupportURL=https://github.com/neochief/savescummer/issues
VersionInfoVersion={#AppVersion}
VersionInfoProductName=SaveScummer
; %LOCALAPPDATA%\Programs\SaveScummer
DefaultDirName={userpf}\SaveScummer
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion={#MinWindows}
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBaseFilename}
SetupIconFile={#RepoRoot}\assets\icon.ico
UninstallDisplayIcon={app}\bin\SaveScummer.exe
UninstallDisplayName=SaveScummer
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
; The Restart Manager closes the UI; the host is shut down gracefully
; first (see PrepareToInstall).
CloseApplications=yes
RestartApplications=no
; An upgrade keeps the user's previous task choices.
UsePreviousTasks=yes

[Tasks]
; Checked on first install. On upgrade UsePreviousTasks restores the previous
; choice, checked or not, so a user who turned it off is never opted back in.
; (Not `checkedonce`: on upgrade it unchecks the task, overriding that choice.)
Name: "autostart"; Description: "Launch at sign-in"

[InstallDelete]
; Upgrades replace the program files wholesale, so nothing stale is left.
Type: filesandordirs; Name: "{app}\bin"

[Files]
Source: "{#Payload}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
; SaveScummer.exe is the host: it shows the UI, or reaches the one running.
Name: "{userprograms}\SaveScummer"; Filename: "{app}\bin\SaveScummer.exe"

[Run]
; The host is the only writer of the sign-in entry.
Filename: "{app}\bin\SaveScummer.exe"; Parameters: "--autostart on"; Flags: runhidden waituntilterminated; Tasks: autostart; StatusMsg: "Setting up launch at sign-in..."
Filename: "{app}\bin\SaveScummer.exe"; Parameters: "--autostart off"; Flags: runhidden waituntilterminated; Tasks: not autostart
; The same as a user launch: the host starts and shows the UI.
Filename: "{app}\bin\SaveScummer.exe"; Description: "Launch SaveScummer"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Removes the entry only if it points at this install.
Filename: "{app}\bin\SaveScummer.exe"; Parameters: "--autostart off"; Flags: runhidden waituntilterminated; RunOnceId: "AutostartOff"

[Code]
{ Asks a running host to shut down, so an in-flight save finishes before its
  files are replaced or removed. The CLI returns once the host has exited. }
procedure ShutDownHost();
var
  Cli: String;
  ResultCode: Integer;
begin
  Cli := ExpandConstant('{app}\bin\SaveScummer.CLI.exe');
  if FileExists(Cli) then
    Exec(Cli, '--no-start shutdown', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  ShutDownHost();
  Result := '';
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    ShutDownHost();
end;
