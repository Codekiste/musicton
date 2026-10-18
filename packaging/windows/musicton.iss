; Inno Setup script for Musicton. Built by packaging\windows\build.ps1 (AppVersion is passed in).
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6F2B9C1E-4D3A-4B8E-9A61-7C2D5E0F1A93}
AppName=Musicton
AppVersion={#AppVersion}
AppPublisher=CodeKiste
AppPublisherURL=https://codekiste.dev
AppSupportURL=https://github.com/Codekiste/musicton
DefaultDirName={autopf}\Musicton
DefaultGroupName=Musicton
DisableProgramGroupPage=yes
LicenseFile=..\..\LICENSE
OutputDir=..\..\dist
OutputBaseFilename=Musicton-{#AppVersion}-windows-x64-setup
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequiredOverridesAllowed=dialog
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\musicton.exe

[Languages]
Name: "german"; MessagesFile: "compiler:Languages\German.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\target\release\musicton.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\target\release\musicton-mcp.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"

[Icons]
Name: "{group}\Musicton"; Filename: "{app}\musicton.exe"
Name: "{autodesktop}\Musicton"; Filename: "{app}\musicton.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\musicton.exe"; Description: "{cm:LaunchProgram,Musicton}"; Flags: nowait postinstall skipifsilent
