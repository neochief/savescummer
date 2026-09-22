; SaveScummer per-user installer (Inno Setup 6.3+).
;
; Do not compile this by hand with different paths; the canonical build is:
;     ./scripts/build-installer.ps1
; which passes SaveScummerVersion, PayloadDir and OutputDir as /D defines.
; The payload is the release portable folder produced by package-windows.ps1.
;
; The installer is strictly per-user (no administrator rights): it installs to
; %LOCALAPPDATA%\Programs\SaveScummer, registers the Explorer integration under
; HKCU and writes the sign-in entry under HKCU. The user's backup data under
; %LOCALAPPDATA%\SaveScummer is never created, moved or removed here.

#ifndef SaveScummerVersion
  #define SaveScummerVersion "0.1.0"
#endif
#ifndef InstallerName
  #define InstallerName "SaveScummer-windows-x64-" + SaveScummerVersion + "-setup"
#endif
#ifndef PayloadDir
  #define PayloadDir "..\..\dist\SaveScummer-windows-x64"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

; Keep these identifiers in sync with:
;   integrations/windows-explorer/identity.h
;   scripts/register-explorer.ps1
#define ExplorerClsid "{{3F8F42CE-463F-41B6-98D1-8C8D16B88931}"

[Setup]
; A stable AppId keeps upgrades in place instead of installing side by side.
AppId={{B7B2AC1E-8E2A-4F2A-9E2B-4E1D9C2A7F10}
AppName=SaveScummer
AppVersion={#SaveScummerVersion}
AppPublisher=SaveScummer contributors
AppPublisherURL=https://github.com/
DefaultDirName={localappdata}\Programs\SaveScummer
DefaultGroupName=SaveScummer
DisableProgramGroupPage=yes
DisableReadyPage=no
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename={#InstallerName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupLogging=yes
UninstallDisplayName=SaveScummer
UninstallDisplayIcon={app}\bin\SaveScummer.exe
CloseApplications=yes
RestartApplications=no
; Code signing slot (D17). Define SignedBuild and configure SignTool in a
; private include to sign the setup and uninstaller; unsigned builds are the
; default and are documented.
; #ifdef SignedBuild
; SignTool=savescummer
; SignedUninstaller=yes
; #endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "explorer"; Description: "Enable the SaveScummer Explorer context menu (Save / Load)"; GroupDescription: "Integration:"; Flags: checkedonce
Name: "startup"; Description: "Launch SaveScummer at sign-in"; GroupDescription: "Integration:"; Flags: unchecked
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; The extension DLL is listed separately so an update can replace a copy that
; explorer.exe still has loaded: Windows cannot overwrite a mapped file in
; place, so restartreplace completes it on restart and Inno asks for one.
Source: "{#PayloadDir}\*"; DestDir: "{app}"; Excludes: "bin\savescummer-explorer.dll"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#PayloadDir}\bin\savescummer-explorer.dll"; DestDir: "{app}\bin"; Flags: ignoreversion restartreplace

[Icons]
Name: "{group}\SaveScummer"; Filename: "{app}\bin\SaveScummer.exe"
Name: "{group}\{cm:UninstallProgram,SaveScummer}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\SaveScummer"; Filename: "{app}\bin\SaveScummer.exe"; Tasks: desktopicon

[Registry]
; Explorer COM registration (per-user). The installer owns these keys for the
; installed copy; scripts/register-explorer.ps1 manages the portable copy.
; The parent CLSID entry carries uninsdeletekey so the whole subtree (including
; InprocServer32) is removed on uninstall, not just the leaf values.
Root: HKCU; Subkey: "Software\Classes\CLSID\{#ExplorerClsid}"; ValueType: none; Flags: uninsdeletekey; Tasks: explorer
Root: HKCU; Subkey: "Software\Classes\CLSID\{#ExplorerClsid}\InprocServer32"; ValueType: string; ValueName: ""; ValueData: "{app}\bin\savescummer-explorer.dll"; Tasks: explorer
Root: HKCU; Subkey: "Software\Classes\CLSID\{#ExplorerClsid}\InprocServer32"; ValueType: string; ValueName: "ThreadingModel"; ValueData: "Apartment"; Tasks: explorer
Root: HKCU; Subkey: "Software\Classes\Directory\shellex\ContextMenuHandlers\SaveScummer"; ValueType: string; ValueName: ""; ValueData: "{#ExplorerClsid}"; Flags: uninsdeletekey; Tasks: explorer
; Sign-in entry. Must match the format written by the host (apps/host / platform
; set_startup): "<host>" --minimized --data-dir "<data>" --desktop "<desktop>".
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "SaveScummer"; ValueData: """{app}\bin\SaveScummer.Host.exe"" --minimized --data-dir ""{localappdata}\SaveScummer"" --desktop ""{app}\bin\SaveScummer.exe"""; Flags: uninsdeletevalue; Tasks: startup

[Run]
Filename: "{app}\bin\SaveScummer.exe"; Description: "Launch SaveScummer"; Flags: nowait postinstall skipifsilent

; The uninstaller removes only what it installed (the application folder and the
; per-user registrations above). %LOCALAPPDATA%\SaveScummer, which holds the
; user's entire backup history, is deliberately left untouched.

[Code]
// Ask any running installed host to shut down gracefully before files are
// replaced, so an accepted save/restore operation can finish. The desktop is
// closed by Inno's Restart Manager (CloseApplications).
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ResultCode: Integer;
  Cli: String;
begin
  Result := '';
  Cli := ExpandConstant('{app}\bin\SaveScummer.CLI.exe');
  if FileExists(Cli) then
  begin
    Exec(Cli, '--no-start shutdown', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
    Sleep(1500);
  end;
end;