; Inno Setup script for XM-Control-Setup-<version>.exe (a per-user install, no admin rights).
; From the repository root (/O is relative to the current directory, SourceExe to this script):
;   ISCC.exe /DAppVersion=0.1.0 /DSourceExe=..\target\release\xm-control.exe /Odist packaging\installer.iss

#ifndef AppVersion
  #error Pass /DAppVersion=x.y.z
#endif
#ifndef SourceExe
  #define SourceExe "..\target\release\xm-control.exe"
#endif

[Setup]
SetupIconFile=..\assets\xm-control.ico
AppId={{3B7E2D41-9C5A-4F86-A0D3-6E1F8B2C7A95}
AppName=XM Control
AppVersion={#AppVersion}
AppVerName=XM Control {#AppVersion}
AppPublisher=Aleksandr Stepanov
AppPublisherURL=https://github.com/Gronck2/xm-control
VersionInfoVersion={#AppVersion}
VersionInfoProductName=XM Control
; Per user: %LOCALAPPDATA%\Programs\XM Control.
PrivilegesRequired=lowest
DefaultDirName={autopf}\XM Control
DisableDirPage=auto
DisableProgramGroupPage=yes
DisableReadyPage=yes
ShowLanguageDialog=no
UsedUserAreasWarning=no
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Windows 10 1607: AdjustWindowRectExForDpi / GetDpiForWindow.
MinVersion=10.0.14393
OutputBaseFilename=XM-Control-Setup-{#AppVersion}
UninstallDisplayIcon={app}\xm-control.exe
UninstallDisplayName=XM Control
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; A running window is closed by the Restart Manager (closing it disconnects cleanly).
CloseApplications=yes

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "xm-control.exe"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\NOTICE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\XM Control"; Filename: "{app}\xm-control.exe"
Name: "{autodesktop}\XM Control"; Filename: "{app}\xm-control.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\xm-control.exe"; Description: "{cm:LaunchProgram,XM Control}"; Flags: nowait postinstall skipifsilent
