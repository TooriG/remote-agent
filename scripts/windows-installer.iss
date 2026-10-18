#ifndef BundleDirectory
  #error BundleDirectory is required
#endif
#ifndef PackageDirectory
  #error PackageDirectory is required
#endif
#ifndef BuildRevision
  #define BuildRevision "preview"
#endif

[Setup]
AppId={{86D79957-8C60-4BB2-AED7-31D423A45B89}
AppName=BEX
AppVersion=0.1.0-{#BuildRevision}
AppPublisher=BEX
AppPublisherURL=https://getbex.dev
DefaultDirName={localappdata}\Programs\BEX
DefaultGroupName=BEX
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.22000
DisableProgramGroupPage=yes
OutputDir={#PackageDirectory}
OutputBaseFilename=bex-windows-x86_64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\bex-desktop.exe
CloseApplications=no
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#BundleDirectory}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\BEX"; Filename: "{app}\bex-desktop.exe"
Name: "{autodesktop}\BEX"; Filename: "{app}\bex-desktop.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\bex-desktop.exe"; Description: "{cm:LaunchProgram,BEX}"; Flags: nowait postinstall skipifsilent
