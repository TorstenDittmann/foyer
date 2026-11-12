; Inno Setup script for the Foyer installer (Foyer-Setup-x64.exe).
;
;   iscc /DAppVersion=0.1.0 /DSourceExe=path\to\Foyer.exe /DOutputDir=dist foyer.iss
;
; Add /DSign and /Sfoyer="<command> $f" to sign the installer and the
; uninstaller with the same command used for Foyer.exe (see sign.ps1).
;
; Foyer installs per user into %LOCALAPPDATA%\Programs\Foyer, so neither the
; install nor its self-updates need administrator rights. The client updates
; itself by running a newer setup with
;   /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CLOSEAPPLICATIONS /relaunch=1
; and the [Run] entry below starts it again afterwards.

#ifndef AppVersion
  #error Pass the version: /DAppVersion=x.y.z
#endif
#ifndef SourceExe
  #define SourceExe "..\..\..\..\target\release\foyer-desktop.exe"
#endif
#ifndef OutputDir
  #define OutputDir "dist"
#endif

[Setup]
; Never change the AppId: Windows uses it to find the existing install.
AppId={{A6BC3941-9184-4289-ADB9-C6C7596B333A}
AppName=Foyer
AppVersion={#AppVersion}
AppVerName=Foyer {#AppVersion}
AppPublisher=Foyer
AppPublisherURL=https://foyer.lol
AppSupportURL=https://foyer.lol/app
AppUpdatesURL=https://foyer.lol/app
VersionInfoVersion={#AppVersion}
DefaultDirName={localappdata}\Programs\Foyer
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=Foyer-Setup-x64
SetupIconFile=..\icon.ico
UninstallDisplayIcon={app}\Foyer.exe
UninstallDisplayName=Foyer
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
#ifdef Sign
SignTool=foyer
SignedUninstaller=yes
#endif

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "Foyer.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Foyer"; Filename: "{app}\Foyer.exe"
Name: "{autodesktop}\Foyer"; Filename: "{app}\Foyer.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Foyer.exe"; Description: "Start Foyer"; Flags: nowait postinstall skipifsilent
Filename: "{app}\Foyer.exe"; Flags: nowait; Check: Relaunch

[Code]
// True when a self-update asked for Foyer to be started again.
function Relaunch: Boolean;
begin
  Result := ExpandConstant('{param:relaunch|0}') = '1';
end;
