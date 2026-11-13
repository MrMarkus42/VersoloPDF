; Windows installer of VersoloPDF (Inno Setup 6).
;
; Built by scripts\make-installer.ps1 from the unpacked release package:
;   ISCC.exe /DAppVersion=1.5.0 /DSourceDir=<unpacked zip> /DOutputDir=<dist> VersoloPDF.iss
;
; * Installs for the current user (no administrator rights) or for all users
;   (Program Files); the setup asks which.
; * Registers VersoloPDF for PDF files and its own session files ("Öffnen
;   mit"). The task "assocpdf" also makes it the program for PDF files where
;   Windows allows that; Windows itself may still ask once.
; * Updates later replace the files in the program folder (see src/update.rs);
;   the uninstaller removes them too.

#ifndef AppVersion
  #error AppVersion is missing (/DAppVersion=x.y.z)
#endif
#ifndef SourceDir
  #error SourceDir is missing (/DSourceDir=<unpacked package>)
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

#define AppExe "VersoloPDF.exe"
#define ProgId "VersoloPDF.Document"
#define SessionProgId "VersoloPDF.Session"

[Setup]
AppId={{97228796-4F3E-4E55-88D3-641F0C1EADD9}
AppName=VersoloPDF
AppVersion={#AppVersion}
AppVerName=VersoloPDF {#AppVersion}
AppPublisher=Mr. M
AppPublisherURL=https://mrmarkus42.github.io/VersoloPDF/
AppSupportURL=https://github.com/MrMarkus42/VersoloPDF/issues
AppUpdatesURL=https://github.com/MrMarkus42/VersoloPDF/releases
VersionInfoVersion={#AppVersion}
VersionInfoProductName=VersoloPDF
DefaultDirName={autopf}\VersoloPDF
DisableProgramGroupPage=yes
; "Nur für mich" (no administrator rights) or "Für alle Benutzer".
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog commandline
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=VersoloPDF-{#AppVersion}-windows-x64-setup
SetupIconFile={#SourcePath}\..\..\assets\versolopdf.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName=VersoloPDF
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes
CloseApplications=yes
RestartApplications=no
ShowLanguageDialog=auto
LicenseFile={#SourceDir}\LICENSE-MIT

[Languages]
Name: "german"; MessagesFile: "compiler:Languages\German.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
german.AssocGroup=Dateizuordnung:
english.AssocGroup=File association:
german.AssocPdf=PDF-Dateien mit VersoloPDF öffnen (Standardprogramm)
english.AssocPdf=Open PDF files with VersoloPDF (default program)
german.PdfDocument=PDF-Dokument
english.PdfDocument=PDF document
german.SessionFile=VersoloPDF-Sitzung
english.SessionFile=VersoloPDF session
german.AppDescription=Kostenloser PDF-Betrachter und -Editor
english.AppDescription=Free PDF viewer and editor

[Tasks]
Name: "assocpdf"; Description: "{cm:AssocPdf}"; GroupDescription: "{cm:AssocGroup}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\VersoloPDF"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\VersoloPDF"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; The document kinds VersoloPDF opens (shown under "Öffnen mit").
Root: HKA; Subkey: "Software\Classes\{#ProgId}"; ValueType: string; ValueName: ""; ValueData: "{cm:PdfDocument}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\{#ProgId}\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"",0"
Root: HKA; Subkey: "Software\Classes\{#ProgId}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""
Root: HKA; Subkey: "Software\Classes\{#SessionProgId}"; ValueType: string; ValueName: ""; ValueData: "{cm:SessionFile}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\{#SessionProgId}\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"",0"
Root: HKA; Subkey: "Software\Classes\{#SessionProgId}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""
Root: HKA; Subkey: "Software\Classes\.pdf\OpenWithProgids"; ValueType: none; ValueName: "{#ProgId}"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.versolopdfsession"; ValueType: string; ValueName: ""; ValueData: "{#SessionProgId}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "VersoloPDF"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\SupportedTypes"; ValueType: string; ValueName: ".pdf"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""
; Listed in Windows' "Standard-Apps".
Root: HKA; Subkey: "Software\VersoloPDF"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\VersoloPDF\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "VersoloPDF"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\VersoloPDF\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "{cm:AppDescription}"
Root: HKA; Subkey: "Software\VersoloPDF\Capabilities\FileAssociations"; ValueType: string; ValueName: ".pdf"; ValueData: "{#ProgId}"
Root: HKA; Subkey: "Software\VersoloPDF\Capabilities\FileAssociations"; ValueType: string; ValueName: ".versolopdfsession"; ValueData: "{#SessionProgId}"
Root: HKA; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "VersoloPDF"; ValueData: "Software\VersoloPDF\Capabilities"; Flags: uninsdeletevalue
; Task: the program for PDF files (Windows keeps a choice the user made).
Root: HKA; Subkey: "Software\Classes\.pdf"; ValueType: string; ValueName: ""; ValueData: "{#ProgId}"; Flags: uninsdeletevalue; Tasks: assocpdf

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,VersoloPDF}"; Flags: nowait postinstall skipifsilent runasoriginaluser

[UninstallRun]
; Removes what VersoloPDF itself registered for this user (Einstellungen ›
; Dateizuordnung).
Filename: "{app}\{#AppExe}"; Parameters: "--unregister-file-types"; Flags: runhidden waituntilterminated; RunOnceId: "UnregisterFileTypes"

[UninstallDelete]
; Left behind by updates (see src/update.rs).
Type: files; Name: "{app}\*.old"
Type: files; Name: "{app}\update-old-files.txt"
Type: filesandordirs; Name: "{app}\licenses"
