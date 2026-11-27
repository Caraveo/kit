[Setup]
AppId=Kit-{#KitArch}
AppName=Kit
AppVersion={#KitVersion}
AppPublisher=Kit Game Engine
DefaultDirName={autopf}\Kit
PrivilegesRequired=admin
ArchitecturesAllowed={#KitArchitecture}
ArchitecturesInstallIn64BitMode={#KitArchitecture}
MinVersion=10.0
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\kit-editor.exe
OutputBaseFilename=kit-{#KitVersion}-windows-{#KitArch}-setup
Compression=lzma2/fast

[Files]
Source: "{#KitRedist}"; DestName: "vc_redist.exe"; Flags: dontcopy noencryption
Source: "{#KitStage}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
#ifdef KitVulkan
Source: "{#KitVulkan}\Kit-Vulkan-Runtime.exe"; Flags: dontcopy noencryption
Source: "{#KitVulkan}\Driver\*"; DestDir: "{app}\software-vulkan"; Flags: ignoreversion
Source: "launch-software-vulkan.cmd"; DestDir: "{app}"; Flags: ignoreversion
#endif

[Icons]
Name: "{commonprograms}\Kit Editor"; Filename: "{app}\kit-editor.exe"; WorkingDir: "{app}"
#ifdef KitVulkan
Name: "{commonprograms}\Kit Editor (Software Vulkan)"; Filename: "{app}\launch-software-vulkan.cmd"; WorkingDir: "{app}"; IconFilename: "{app}\kit-editor.exe"
#endif

[Code]
var
  RuntimeNeedsRestart: Boolean;

#ifdef KitVulkan
function PrepareVulkan(): String;
var
  InstallerArgs: String;
  ExitCode: Integer;
begin
  Result := '';
  if FileExists(ExpandConstant('{sys}\vulkan-1.dll')) then
  begin
    if WizardSilent then
      Exit;
    if TaskDialogMsgBox('Vulkan runtime is already installed',
      'Keep your current runtime or run the bundled LunarG installer again.',
      mbConfirmation, MB_YESNO, ['Reinstall', 'Skip'], 0) <> IDYES then
      Exit;
  end;

  InstallerArgs := '';
  if WizardSilent then
    InstallerArgs := '--accept-licenses --default-answer --confirm-command install';
  ExtractTemporaryFile('Kit-Vulkan-Runtime.exe');
  if not Exec(ExpandConstant('{tmp}\Kit-Vulkan-Runtime.exe'), InstallerArgs, '',
    SW_SHOW, ewWaitUntilTerminated, ExitCode) then
    Result := 'Could not launch the Vulkan runtime installer. Windows error ' + IntToStr(ExitCode) + '.'
  else if ExitCode = 3010 then
    RuntimeNeedsRestart := True
  else if (ExitCode <> 0) or not FileExists(ExpandConstant('{sys}\vulkan-1.dll')) then
    Result := 'The Vulkan runtime setup did not complete. Exit code ' + IntToStr(ExitCode) + '.';
end;
#endif

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ExitCode: Integer;
begin
  Result := '';
  ExtractTemporaryFile('vc_redist.exe');
  if not Exec(ExpandConstant('{tmp}\vc_redist.exe'), '/install /quiet /norestart',
    '', SW_HIDE, ewWaitUntilTerminated, ExitCode) then
    Result := 'Could not launch the Microsoft Visual C++ installer. Windows error ' + IntToStr(ExitCode) + '.'
  else
    case ExitCode of
      0, 1638: ; { Installed successfully, or a newer runtime is already installed. }
      3010: RuntimeNeedsRestart := True;
    else
      Result := 'Microsoft Visual C++ installation failed. Exit code ' + IntToStr(ExitCode) + '.';
    end;
  #ifdef KitVulkan
  if Result = '' then
    Result := PrepareVulkan();
  #endif
end;

function NeedRestart(): Boolean;
begin
  Result := RuntimeNeedsRestart;
end;
