; OmniType FreePTT — Windows installer
; -----------------------------------------------------------------------
; Build with:
;   "C:\Program Files (x86)\Inno Setup 6\ISCC.exe" installer\omnitype.iss
;
; The output name and the version are derived from the crate rather than typed
; in twice. A release whose filename disagrees with the version inside it is a
; release the updater cannot compare — `is_newer_version` reads the tag, and the
; banner links to a file named after a different number.
;
; The installer ships ONLY the exe. Config, dictionary, models and history live
; in %AppData% and are untouched by an upgrade, so reinstalling never costs the
; user a dictionary or a downloaded model.

#define AppName "OmniType FreePTT"
#define AppExeName "voice-ptt.exe"
#define AppPublisher "OmniType"
#define AppURL "https://github.com/mahdimoslemi88-sys/OmniType-FreePTT-2"

; The version is passed in by the build, not parsed here.
;
; An ISPP macro was tried first and produced filenames like
; `51061184-setup` and `55643536-setup`: Cargo.toml is CRLF on this machine
; and the parse went wrong quietly. Nothing else in the build complained,
; because the .exe inside was still correct — the only symptom was a filename
; that matched no release.
;
; So the caller does the reading (one line of grep on Cargo.toml) and asserts the
; result looks like a version before handing it over. build-installer.cmd does
; exactly that and fails the build if the string is not numeric.
#ifndef AppVersion
  #error AppVersion must be passed on the command line, e.g. -DAppVersion=0.4.0
#endif

[Setup]
AppId={{6B1F4C2A-9D3E-4A57-8C11-2F0E7B5D9041}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}
AppUpdatesURL={#AppURL}
DefaultDirName={autopf}\OmniType FreePTT
DefaultGroupName={#AppName}
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\{#AppExeName}
OutputDir=Output
OutputBaseFilename=OmniType-FreePTT-{#AppVersion}-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; Per-user install. The previous releases were described as "no UAC required",
; and that promise is kept by not asking for elevation.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesInstallIn64BitMode=x64compatible
DisableProgramGroupPage=yes
Uninstallable=yes
SetupLogging=yes
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "persian"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "autostart"; Description: "اجرای خودکار OmniType هنگام شروع ویندوز"; GroupDescription: "ویندوز:"; Flags: unchecked
Name: "desktopicon"; Description: "ساخت میان‌بر روی دسکتاپ"; GroupDescription: "ویندوز:"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; \
  ValueType: string; ValueName: "OmniType FreePTT"; \
  ValueData: """{app}\{#AppExeName}"""; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\{#AppExeName}"; Description: "اجرای {#AppName}"; \
  Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Never delete user data. An uninstall that took the dictionary or the config
; with it would be a data-loss bug disguised as tidiness. The uninstaller only
; removes what this installer wrote.
Type: filesandordirs; Name: "{app}"

[Code]
function InitializeSetup(): Boolean;
begin
  Result := True;
end;