; Installer za Kasa Forwarder (Inno Setup 6.5+ / 7).
; Gradi se preko tools\napravi-installer.ps1, koji prosljeđuje verziju iz Cargo.toml.
;
; Instalira se po korisniku, bez admin prava, u %LOCALAPPDATA%\Programs\Kasa Forwarder,
; jer forwarder pored exe-a piše config.json i log. Tiha instalacija za više kasa:
;   KasaForwarder-Setup-X.Y.Z.exe /VERYSILENT /TASKS="autostart"

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#define AppName "Kasa Forwarder"
#define AppExe "kasa-forwarder.exe"
#define RunKljuc "Software\Microsoft\Windows\CurrentVersion\Run"
; Isto ime vrijednosti koristi i sam program (install/uninstall, prekidač u prozoru).
#define RunVrijednost "KasaForwarder"

[Setup]
; AppId se nikad ne mijenja — po njemu installer prepoznaje nadogradnju.
AppId={{8B4E2C1A-5F3D-4A7E-9C21-6D0F4B8A2E17}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Tarik Caplja
VersionInfoVersion={#AppVersion}
VersionInfoDescription={#AppName} Setup
PrivilegesRequired=lowest
DefaultDirName={autopf}\{#AppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=KasaForwarder-Setup-{#AppVersion}
SetupIconFile=..\assets\ikona.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern
WizardImageFile=wizard-veliki-164.bmp,wizard-veliki-246.bmp,wizard-veliki-328.bmp
WizardSmallImageFile=wizard-mali-55.bmp,wizard-mali-83.bmp,wizard-mali-110.bmp
Compression=lzma2/max
SolidCompression=yes
; Program gasimo sami (PrepareToInstall), Restart Manager ne zna za tray proces.
CloseApplications=no

[Languages]
; Default.isl prvi: poruke koje hrvatski prevod nema ostaju engleske umjesto greške.
Name: "hr"; MessagesFile: "compiler:Default.isl,Croatian.isl"

[CustomMessages]
hr.Dodatno=Dodatno:
hr.Autostart=Pokreni automatski pri prijavi na Windows (preporučeno)
hr.Pokreni=Pokreni Kasa Forwarder
hr.Opis=Log i podešavanja Kasa Forwardera
hr.ObrisatiPodatke=Obrisati i podešavanja (config.json, token, adrese) i log?%n%nAko planiraš ponovo instalirati forwarder, odaberi Ne.

[Tasks]
Name: "autostart"; Description: "{cm:Autostart}"; GroupDescription: "{cm:Dodatno}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; Comment: "{cm:Opis}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Comment: "{cm:Opis}"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "{#RunKljuc}"; ValueType: string; ValueName: "{#RunVrijednost}"; \
  ValueData: """{app}\{#AppExe}"" --autostart"; Tasks: autostart; Flags: uninsdeletevalue

[Run]
; Obična instalacija: kvačica na kraju pokreće forwarder i otvara njegov prozor.
Filename: "{app}\{#AppExe}"; Description: "{cm:Pokreni}"; Flags: nowait postinstall skipifsilent
; Tiha instalacija: pokreni u pozadini, bez prozora.
Filename: "{app}\{#AppExe}"; Parameters: "--autostart"; Flags: nowait; Check: WizardSilent

[Code]
const
  ExeIme = '{#AppExe}';

function ForwarderRadi(): Boolean;
var
  Kod: Integer;
begin
  Result := Exec(ExpandConstant('{cmd}'),
    '/C tasklist /FI "IMAGENAME eq ' + ExeIme + '" /NH | find /I "' + ExeIme + '" >nul',
    '', SW_HIDE, ewWaitUntilTerminated, Kod) and (Kod = 0);
end;

{ Prvo uredno (WM_CLOSE — forwarder skine ikonu iz traya i upiše STOP u log), pa na silu. }
procedure ZatvoriForwarder();
var
  Kod, I: Integer;
begin
  if not ForwarderRadi() then
    Exit;
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM ' + ExeIme, '', SW_HIDE, ewWaitUntilTerminated, Kod);
  for I := 1 to 20 do
  begin
    if not ForwarderRadi() then
      Exit;
    Sleep(250);
  end;
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM ' + ExeIme, '', SW_HIDE, ewWaitUntilTerminated, Kod);
  Sleep(500);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  ZatvoriForwarder();
  Result := '';
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  { Izbor u installeru ima prednost i nad prekidačem iz prozora forwardera. }
  if (CurStep = ssPostInstall) and not WizardIsTaskSelected('autostart') then
    RegDeleteValue(HKCU, '{#RunKljuc}', '{#RunVrijednost}');
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Dir: String;
begin
  if CurUninstallStep = usUninstall then
  begin
    ZatvoriForwarder();
    { Autostart je možda uključen iz prozora forwardera, ne iz installera. }
    RegDeleteValue(HKCU, '{#RunKljuc}', '{#RunVrijednost}');
  end;
  if CurUninstallStep = usPostUninstall then
  begin
    Dir := ExpandConstant('{app}');
    if not UninstallSilent and
       (MsgBox(CustomMessage('ObrisatiPodatke'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES) then
    begin
      DeleteFile(Dir + '\config.json');
      DeleteFile(Dir + '\kasa-forwarder.log');
      DeleteFile(Dir + '\kasa-forwarder.log.1');
    end;
    RemoveDir(Dir);
  end;
end;
