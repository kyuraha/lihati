; Lihati installer - build with Inno Setup 6 (https://jrsoftware.org/isinfo.php)
;   iscc packaging\lihati-setup.iss
; Per-user install, no admin rights required.

#define AppName "Lihati"
#define AppVersion "0.1.0"
#define AppExe "lihati.exe"

[Setup]
AppId={{7E1A2C34-9B5D-4E6F-8A21-LIHATI0000001}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Lihati
DefaultDirName={localappdata}\Programs\Lihati
DefaultGroupName=Lihati
DisableProgramGroupPage=yes
OutputDir=dist
OutputBaseFilename=Lihati-setup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\{#AppExe}

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Lihati"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\Lihati"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Parameters: "--register"; Flags: runhidden skipifsilent
Filename: "{app}\{#AppExe}"; Description: "Launch Lihati"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\{#AppExe}"; Parameters: "--unregister"; Flags: runhidden; RunOnceId: "UnregisterAssoc"
