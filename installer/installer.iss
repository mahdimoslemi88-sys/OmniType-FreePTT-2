; Inno Setup 6 script for OmniType FreePTT (voice-ptt)
; Build:  ISCC.exe installer.iss   →  Output\OmniType-FreePTT-<ver>-setup.exe
; Source layout is the release dist folder (../voice-ptt-dist).

#define MyAppName "OmniType FreePTT"
#define MyAppVersion "0.1.0"
#define MyAppPublisher "OmniType"
#define MyAppExeName "voice-ptt.exe"
#define DistRoot "..\voice-ptt-dist"

[Setup]
AppId={{D4A7B8E1-3C9F-4E62-9B5D-7A1C2F8E4B60}}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
; Per-user install (like VS Code's user setup): no UAC prompt, and the
; optional autostart task lands in *this user's* Startup folder. The app
; already keeps its runtime data per-user in %APPDATA%\voice-ptt.
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\OmniType FreePTT
DefaultGroupName=OmniType FreePTT
DisableProgramGroupPage=yes
OutputDir=Output
OutputBaseFilename=OmniType-FreePTT-{#MyAppVersion}-setup
SetupIconFile=..\icon.ico
Compression=lzma2/fast
SolidCompression=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes
UninstallDisplayIcon={app}\{#MyAppExeName}

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; \
    GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "autostart"; Description: "Start OmniType FreePTT when Windows starts"; \
    GroupDescription: "Startup:"; Flags: unchecked

[Types]
Name: "full"; Description: "Full installation (includes the large model)"
Name: "compact"; Description: "Compact installation (base model only)"
Name: "custom"; Description: "Custom"; Flags: iscustom

[Components]
Name: "core"; Description: "OmniType FreePTT (app, DirectML, VAD asset, dictionary)"; \
    Types: full compact custom; Flags: fixed
Name: "models/base"; Description: "Whisper base model (~141 MB) — fast, English+multilingual"; \
    Types: full compact
Name: "models/large"; Description: "Whisper large-v3-turbo model (~1.5 GB) — highest accuracy"; \
    Types: full

[Files]
Source: "{#DistRoot}\{#MyAppExeName}"; DestDir: "{app}"; \
    Flags: ignoreversion; Components: core
Source: "{#DistRoot}\DirectML.dll"; DestDir: "{app}"; \
    Flags: ignoreversion; Components: core
Source: "{#DistRoot}\icon.ico"; DestDir: "{app}"; \
    Flags: ignoreversion; Components: core
Source: "{#DistRoot}\dictionary.toml"; DestDir: "{app}"; \
    Flags: ignoreversion onlyifdoesntexist; Components: core
Source: "{#DistRoot}\README-TEST.md"; DestDir: "{app}"; \
    Flags: ignoreversion; Components: core
Source: "{#DistRoot}\assets\silero_vad.onnx"; DestDir: "{app}\assets"; \
    Flags: ignoreversion; Components: core
Source: "{#DistRoot}\models\ggml-base.bin"; DestDir: "{app}\models"; \
    Flags: ignoreversion; Components: models/base
Source: "{#DistRoot}\models\ggml-large-v3-turbo.bin"; DestDir: "{app}\models"; \
    Flags: ignoreversion; Components: models/large

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; \
    Tasks: desktopicon
Name: "{userstartup}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; \
    Tasks: autostart

[Run]
; The finished-page launch checkbox is the bootstrap trigger: the custom
; "Model Preparation" page pre-ticks it, and the app itself downloads the
; missing Whisper model on first startup.
Filename: "{app}\{#MyAppExeName}"; \
    Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; \
    Flags: nowait postinstall skipifsilent runasoriginaluser

[UninstallRun]
Filename: "taskkill.exe"; Parameters: "/F /IM {#MyAppExeName}"; \
    Flags: runhidden; RunOnceId: "KillVoicePtt"

[UninstallDelete]
; Model weights (installed or later downloaded by the app) are re-downloadable;
; remove them so an uninstall actually frees the ~1.7 GB.
Type: filesandordirs; Name: "{app}\models"
Type: files; Name: "{app}\DirectML.dll"

[Messages]
WelcomeLabel2=This will install [name/ver] on your computer.%n%nAn offline, push-to-talk voice typing tool: hold the hotkey, speak, release, and the text is typed into any app.%n%nIt is recommended that you close all other applications before continuing.
SelectComponentsLabel2=Which Whisper model should be installed?%n%nYou can skip both and download models later from inside the app; a model installed here works immediately, fully offline.
SelectTasksLabel2=Which additional shortcuts should be created?%n%n"Start when Windows starts" makes the push-to-talk capsule available right after login.
FinishedHeadingLabel=[name] has been installed
FinishedLabelNoIcons=[name] has been installed on your computer.%n%nHold the hotkey (CapsLock by default) anywhere, speak, then release to type your text.
FinishedLabel=[name] has been installed on your computer.%n%nHold the hotkey (CapsLock by default) anywhere, speak, then release to type your text.

[Code]
var
  ModelPrepPage: TInputOptionWizardPage;
  // Zero-initialized (False): silent installs never auto-launch anything.
  BootstrapLaunchWanted: Boolean;

function NoModelsSelected: Boolean;
begin
  Result :=
    (not WizardIsComponentSelected('models/base')) and
    (not WizardIsComponentSelected('models/large'));
end;

procedure InitializeWizard;
begin
  // Shown only when the user picked no model: the app downloads its first
  // Whisper model automatically on startup, so "preparing the model" here
  // simply means launching the app once after installation.
  ModelPrepPage := CreateInputOptionPage(wpSelectComponents,
    'Model Preparation', 'No Whisper model was selected',
    'OmniType needs one Whisper model to transcribe. None was installed, so on '
    + 'first launch the app will download it automatically (needs internet; '
    + 'the base model is ~141 MB, large-v3-turbo is ~1.5 GB).', False, False);
  ModelPrepPage.Add('&Start OmniType after installation to download the model now');
  ModelPrepPage.Values[0] := True;
end;

function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := False;
  if (PageID = ModelPrepPage.ID) and (not NoModelsSelected) then
    Result := True;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if CurPageID = ModelPrepPage.ID then
  begin
    BootstrapLaunchWanted := ModelPrepPage.Values[0];
    if BootstrapLaunchWanted and (WizardForm.RunList.Items.Count > 0) then
      // Make sure the finished-page launch box is ticked so the download
      // actually starts when the user clicks Finish.
      WizardForm.RunList.Checked[0] := True;
  end;
end;

function ModelBootstrapWanted: Boolean;
begin
  Result := BootstrapLaunchWanted and NoModelsSelected;
end;
