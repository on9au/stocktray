; Per-user installer, no admin needed. Build with package.ps1, which passes the version from
; Cargo.toml: iscc /DAppVersion=x.y.z installer\stocktray.iss

#ifndef AppVersion
  #error Pass the version: iscc /DAppVersion=x.y.z installer\stocktray.iss
#endif

#define AppExe "stocktray.exe"
#define RunKey "Software\Microsoft\Windows\CurrentVersion\Run"

[Setup]
; Never change AppId: it's how an update finds the installed copy.
AppId={{4ED599FD-6F71-41E0-A0E7-CBD1DD7821CB}
AppName=stocktray
AppVersion={#AppVersion}
AppPublisher=on9au
AppPublisherURL=https://github.com/on9au/stocktray
PrivilegesRequired=lowest
; {autopf} is %LOCALAPPDATA%\Programs for a per-user install.
DefaultDirName={autopf}\stocktray
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; The app's updater looks for this name: see installer_name in src/update.rs.
OutputBaseFilename=stocktray-{#AppVersion}-setup
OutputDir=..\dist
SetupIconFile=..\assets\stocktray.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName=stocktray
WizardStyle=modern
; Stop a running copy rather than asking to reboot.
CloseApplications=force

[Tasks]
Name: autostart; Description: "Start stocktray when I sign in"

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\stocktray"; Filename: "{app}\{#AppExe}"

[Registry]
; The same value the app's "Start with Windows" item sets. Silent installs are the app updating
; itself, so they leave whatever was chosen there alone.
Root: HKCU; Subkey: "{#RunKey}"; ValueType: string; ValueName: "stocktray"; ValueData: """{app}\{#AppExe}"""; Tasks: autostart; Check: not WizardSilent

[Run]
Filename: "{app}\{#AppExe}"; Description: "Start stocktray"; Flags: nowait postinstall skipifsilent
; The app exits as its update installs, so start the new version.
Filename: "{app}\{#AppExe}"; Flags: nowait; Check: WizardSilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM {#AppExe}"; Flags: runhidden; RunOnceId: "StopStocktray"

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  // The app may have set this itself, so it isn't covered by [Registry].
  if CurUninstallStep = usUninstall then
    RegDeleteValue(HKCU, '{#RunKey}', 'stocktray');
end;
