; Inno Setup script for the Windows installer. Built in CI by windows.yml:
;
;   iscc /DAppVersion=0.2.5 crates\app\installer\agent-workspace.iss
;
; Installs per user (no admin prompt) into one fixed folder, so a new version overwrites the old
; one in place and taskbar pins and Start Menu shortcuts keep opening the current version.

#ifndef AppVersion
  #error Pass the version: iscc /DAppVersion=X.Y.Z agent-workspace.iss
#endif

[Setup]
; Never change the AppId: it is how a new version finds and upgrades the installed one.
AppId={{61E95B5A-563C-45D5-9848-146AC2F3D8C1}
AppName=Agent Workspace
AppVersion={#AppVersion}
AppVerName=Agent Workspace {#AppVersion}
AppPublisher=Agent Workspace
; {autopf} is %LOCALAPPDATA%\Programs for a per-user install.
DefaultDirName={autopf}\Agent Workspace
DisableDirPage=yes
DefaultGroupName=Agent Workspace
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Quit a running copy before replacing its files.
CloseApplications=yes
RestartApplications=no
SetupIconFile=..\assets\app-icon\agent-workspace.ico
UninstallDisplayIcon={app}\agent-workspace.exe
OutputDir=..\..\..
OutputBaseFilename=agent-workspace-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: desktopicon; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; agentws-hook must sit next to the app, or agent status is disabled.
Source: "..\..\..\target\release\agent-workspace.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\..\target\release\agentws-hook.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Agent Workspace"; Filename: "{app}\agent-workspace.exe"
Name: "{autodesktop}\Agent Workspace"; Filename: "{app}\agent-workspace.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\agent-workspace.exe"; Description: "{cm:LaunchProgram,Agent Workspace}"; Flags: nowait postinstall skipifsilent
