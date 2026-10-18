; Inno Setup script for the B-303 plug-in. Installs VST3 and CLAP to the standard system folders.
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{A1C4E7B2-3F5D-4E9A-8B06-2D7F9C1E5B48}
AppName=B-303 Bassline
AppVersion={#AppVersion}
AppPublisher=CodeKiste
AppPublisherURL=https://codekiste.dev
AppSupportURL=https://github.com/Codekiste/musicton
DefaultDirName={commoncf64}\CodeKiste\B-303
DisableDirPage=yes
DisableProgramGroupPage=yes
LicenseFile=..\..\LICENSE
OutputDir=..\..\dist
OutputBaseFilename=B-303-{#AppVersion}-windows-x64-setup
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "german"; MessagesFile: "compiler:Languages\German.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Components]
Name: "vst3"; Description: "VST3-Plugin"; Types: full compact custom; Flags: checkablealone
Name: "clap"; Description: "CLAP-Plugin"; Types: full custom; Flags: checkablealone

[Files]
Source: "..\..\target\bundled\B-303.vst3\*"; DestDir: "{commoncf64}\VST3\B-303.vst3"; Components: vst3; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\..\target\bundled\B-303.clap"; DestDir: "{commoncf64}\CLAP"; Components: clap; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"
