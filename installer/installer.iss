; Inno Setup 6 script for OmniType FreePTT (voice-ptt)
; Build:  ISCC.exe installer.iss   →  Output\OmniType-FreePTT-<ver>-setup.exe
; A LIGHT setup (~35 MB): the app only. Whisper models are NOT bundled —
; the wizard downloads the selected model with progress + SHA-256
; verification, and if the user skips that, the app downloads a model on
; first launch. For offline media, use the voice-ptt-dist folder directly.

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

[Files]
; App payload — the entire light setup (~35 MB).
Source: "{#DistRoot}\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#DistRoot}\DirectML.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#DistRoot}\icon.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#DistRoot}\dictionary.toml"; DestDir: "{app}"; \
    Flags: ignoreversion onlyifdoesntexist
Source: "{#DistRoot}\README-TEST.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#DistRoot}\assets\silero_vad.onnx"; DestDir: "{app}\assets"; Flags: ignoreversion
; Whisper models downloaded in-wizard (see [Code]) into {tmp}, then copied
; here — the app resolves models exe-first, so {app}\models wins. Same URLs
; and hashes as the app's own downloader (asr/downloader.rs).
Source: "{tmp}\ggml-base.bin"; DestDir: "{app}\models"; \
    Flags: external ignoreversion; Check: ModelWanted('base')
Source: "{tmp}\ggml-large-v3-turbo.bin"; DestDir: "{app}\models"; \
    Flags: external ignoreversion; Check: ModelWanted('large')

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; \
    Tasks: desktopicon
Name: "{userstartup}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; \
    Tasks: autostart

[Run]
; Launch checkbox on the finished page: launching the app right after
; install also exercises its first-launch model download when the user
; chose "None" on the model page.
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
SelectTasksLabel2=Which additional shortcuts should be created?%n%n"Start when Windows starts" makes the push-to-talk capsule available right after login.
FinishedHeadingLabel=[name] has been installed
FinishedLabelNoIcons=[name] has been installed on your computer.%n%nHold the hotkey (CapsLock by default) anywhere, speak, then release to type your text.
FinishedLabel=[name] has been installed on your computer.%n%nHold the hotkey (CapsLock by default) anywhere, speak, then release to type your text.

[Code]
const
  // Same URLs the app's own downloader uses (asr/downloader.rs), so a
  // model fetched here is byte-identical to one the app would fetch.
  ModelBaseURL = 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/';
  // SHA-256 of each model, computed from the packaged dist files
  // (byte-identical to the HuggingFace artifacts).
  BaseSHA256 = '60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe';
  LargeSHA256 = '1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69';

var
  ModelPage: TInputOptionWizardPage;
  DLPage: TDownloadWizardPage;

// Writes {app}\config.toml pinning the model chosen on the wizard's model
// page. The app resolves config exe-first, so this BEATS any pre-existing
// %APPDATA%\voice-ptt\config.toml — without it the app's default "auto"
// policy would pick large-v3-turbo on GPU machines and silently download
// 1.5 GB regardless of what the user picked here. Pure ASCII on purpose:
// SaveStringsToUTF8File would emit a BOM that Rust's TOML parser rejects.
procedure WriteModelConfig(ModelName: String);
var
  S: String;
begin
  S := '[asr]' #13#10 +
       'model = "' + ModelName + '"' #13#10 +
       'language = "fa"' #13#10;
  SaveStringToFile(S, ExpandConstant('{app}\config.toml'), False);
end;

procedure InitializeWizard;
begin
  // Model choice right after the directory page. "None" leaves the
  // download to the app's own first-launch logic.
  ModelPage := CreateInputOptionPage(wpSelectDir,
    'Which Whisper model?', 'Downloaded during installation',
    'Pick the speech-recognition model to download now (internet required). '
    + 'You can also skip this: on first launch the app downloads a model '
    + 'automatically.', False, False);
  ModelPage.Add('&Whisper base (~141 MB) — fast, recommended');
  ModelPage.Add('Whisper &large-v3-turbo (~1.5 GB) — highest accuracy');
  ModelPage.Add('&None now — the app downloads a model on first launch');
  ModelPage.Values[0] := True;

  DLPage := CreateDownloadPage(SetupMessage(msgWizardPreparing),
    SetupMessage(msgPreparingDesc), nil);
  DLPage.ShowBaseNameInsteadOfUrl := True;
end;

// [Files] Check hook: which model entries got downloaded.
function ModelWanted(Which: String): Boolean;
begin
  if Which = 'base' then
    Result := ModelPage.Values[0]
  else if Which = 'large' then
    Result := ModelPage.Values[1]
  else
    Result := False;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
var
  Error: String;
begin
  Result := True;
  if (CurPageID = wpReady) and (not ModelPage.Values[2]) then
  begin
    // Download the chosen model now, with progress + hash verification.
    // Failure keeps the user on the Ready page to retry or cancel.
    DLPage.Clear;
    if ModelPage.Values[0] then
      DLPage.Add(ModelBaseURL + 'ggml-base.bin', 'ggml-base.bin', BaseSHA256);
    if ModelPage.Values[1] then
      DLPage.Add(ModelBaseURL + 'ggml-large-v3-turbo.bin',
        'ggml-large-v3-turbo.bin', LargeSHA256);
    DLPage.Show;
    try
      try
        DLPage.Download;
        // Clear any stale partial download left by an earlier interrupted
        // first launch of the app before it tries the same model itself.
        DeleteFile(ExpandConstant('{app}\models\ggml-base.part'));
        DeleteFile(ExpandConstant('{app}\models\ggml-large-v3-turbo.part'));
        Result := True;
      except
        if DLPage.AbortedByUser then
          Log('Model download aborted by user.')
        else
        begin
          Error := Format('%s: %s', [DLPage.LastBaseNameOrUrl, GetExceptionMessage]);
          SuppressibleMsgBox(AddPeriod(Error), mbCriticalError, MB_OK, IDOK);
        end;
        Result := False;
      end;
    finally
      DLPage.Hide;
    end;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  AppConfig: String;
begin
  if CurStep = ssInstall then
  begin
    // Only a FRESH install pins the wizard choice: an upgrade must keep
    // whatever model the user already configured (in {app} or %APPDATA%),
    // or it would silently switch their model on every update.
    AppConfig := ExpandConstant('{app}\config.toml');
    if (not FileExists(AppConfig))
       and (not FileExists(ExpandConstant('{userappdata}\voice-ptt\config.toml'))) then
    begin
      if ModelPage.Values[0] then
        WriteModelConfig('base')
      else if ModelPage.Values[1] then
        WriteModelConfig('large-v3-turbo')
      else
        WriteModelConfig('auto');
    end;
  end;
end;
