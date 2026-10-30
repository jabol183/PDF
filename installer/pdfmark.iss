; Inno Setup script for PDFMark. Build with: iscc installer\pdfmark.iss
; Expects the files to install in dist\PDFMark\ (see the release workflow).

#ifndef AppVersion
  #define AppVersion "1.1.0"
#endif

[Setup]
AppId={{6B8F3E2A-5C1D-4E7B-9A40-2F1D7C3B9E51}
AppName=PDFMark
AppVersion={#AppVersion}
AppVerName=PDFMark {#AppVersion}
AppPublisher=PDFMark contributors
DefaultDirName={autopf}\PDFMark
DefaultGroupName=PDFMark
DisableProgramGroupPage=yes
; Installs for the current user without admin rights; admins may choose all users.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=PDFMark-Setup-{#AppVersion}-x64
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\PDFMark.exe
LicenseFile=..\LICENSE
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "pdfopenwith"; Description: "Add PDFMark to the ""Open with"" menu for PDF files"; GroupDescription: "File types:"

[Files]
Source: "..\dist\PDFMark\PDFMark.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\PDFMark\pdfium.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\PDFMark\LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\PDFMark\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\PDFMark\licenses\*"; DestDir: "{app}\licenses"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{autoprograms}\PDFMark"; Filename: "{app}\PDFMark.exe"
Name: "{autodesktop}\PDFMark"; Filename: "{app}\PDFMark.exe"; Tasks: desktopicon

[Registry]
; .pdfmark editable projects open in PDFMark.
Root: HKA; Subkey: "Software\Classes\.pdfmark"; ValueType: string; ValueName: ""; ValueData: "PDFMark.Project"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\PDFMark.Project"; ValueType: string; ValueName: ""; ValueData: "PDFMark editable project"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\PDFMark.Project\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\PDFMark.exe,0"
Root: HKA; Subkey: "Software\Classes\PDFMark.Project\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\PDFMark.exe"" ""%1"""
; PDFs: offer PDFMark in "Open with" without taking over the default viewer.
Root: HKA; Subkey: "Software\Classes\PDFMark.PDF"; ValueType: string; ValueName: ""; ValueData: "PDF document (PDFMark)"; Flags: uninsdeletekey; Tasks: pdfopenwith
Root: HKA; Subkey: "Software\Classes\PDFMark.PDF\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\PDFMark.exe,0"; Tasks: pdfopenwith
Root: HKA; Subkey: "Software\Classes\PDFMark.PDF\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\PDFMark.exe"" ""%1"""; Tasks: pdfopenwith
Root: HKA; Subkey: "Software\Classes\.pdf\OpenWithProgids"; ValueType: string; ValueName: "PDFMark.PDF"; ValueData: ""; Flags: uninsdeletevalue; Tasks: pdfopenwith
Root: HKA; Subkey: "Software\Classes\Applications\PDFMark.exe\SupportedTypes"; ValueType: string; ValueName: ".pdf"; ValueData: ""; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\PDFMark.exe\SupportedTypes"; ValueType: string; ValueName: ".pdfmark"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\PDFMark.exe\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\PDFMark.exe"" ""%1"""

[Run]
Filename: "{app}\PDFMark.exe"; Description: "{cm:LaunchProgram,PDFMark}"; Flags: nowait postinstall skipifsilent
