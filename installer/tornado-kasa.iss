; Installer za Tornado Kasa (Inno Setup 6.5+ / 7).
; Gradi se preko tools\napravi-installer.ps1, koji prosljeđuje verziju iz Cargo.toml.
;
; Instalira se po korisniku, bez admin prava, u %LOCALAPPDATA%\Programs\Tornado Kasa,
; jer forwarder pored exe-a piše config.json i log. Tiha instalacija za više kasa:
;   TornadoKasa-Setup-X.Y.Z.exe /VERYSILENT /TASKS="autostart"

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#define AppName "Tornado Kasa"
#define AppExe "tornado-kasa.exe"
#define RunKljuc "Software\Microsoft\Windows\CurrentVersion\Run"
; Isto ime vrijednosti koristi i sam program (install/uninstall, prekidač u prozoru).
#define RunVrijednost "TornadoKasa"
; Do 0.1.3 program se zvao Kasa Forwarder. Nadogradnja ide u isti folder (isti AppId),
; pa stari exe, autostart i log treba počistiti/preimenovati.
#define StariExe "kasa-forwarder.exe"
#define StariRun "KasaForwarder"
#define StariLog "kasa-forwarder.log"
#define Log "tornado-kasa.log"

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
OutputBaseFilename=TornadoKasa-Setup-{#AppVersion}
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
hr.Pokreni=Pokreni Tornado Kasu
hr.Opis=Log i podešavanja Tornado Kase
hr.ObrisatiPodatke=Obrisati i podešavanja (config.json, token, adrese) i log?%n%nAko planiraš ponovo instalirati forwarder, odaberi Ne.

[Tasks]
Name: "autostart"; Description: "{cm:Autostart}"; GroupDescription: "{cm:Dodatno}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion

[InstallDelete]
Type: files; Name: "{app}\{#StariExe}"

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
function ProcesRadi(ExeIme: String): Boolean;
var
  Kod: Integer;
begin
  Result := Exec(ExpandConstant('{cmd}'),
    '/C tasklist /FI "IMAGENAME eq ' + ExeIme + '" /NH | find /I "' + ExeIme + '" >nul',
    '', SW_HIDE, ewWaitUntilTerminated, Kod) and (Kod = 0);
end;

{ Prvo uredno (WM_CLOSE — program skine ikonu iz traya i upiše STOP u log), pa na silu. }
procedure ZatvoriProces(ExeIme: String);
var
  Kod, I: Integer;
begin
  if not ProcesRadi(ExeIme) then
    Exit;
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM ' + ExeIme, '', SW_HIDE, ewWaitUntilTerminated, Kod);
  for I := 1 to 20 do
  begin
    if not ProcesRadi(ExeIme) then
      Exit;
    Sleep(250);
  end;
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM ' + ExeIme, '', SW_HIDE, ewWaitUntilTerminated, Kod);
  Sleep(500);
end;

procedure ZatvoriForwarder();
begin
  ZatvoriProces('{#AppExe}');
  ZatvoriProces('{#StariExe}');
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  ZatvoriForwarder();
  Result := '';
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Dir: String;
begin
  if CurStep = ssPostInstall then
  begin
    { Izbor u installeru ima prednost i nad prekidačem iz prozora forwardera. }
    if not WizardIsTaskSelected('autostart') then
      RegDeleteValue(HKCU, '{#RunKljuc}', '{#RunVrijednost}');
    { Ostaci Kasa Forwardera: stari autostart bi pokretao obrisani exe, log zadržavamo pod novim imenom. }
    RegDeleteValue(HKCU, '{#RunKljuc}', '{#StariRun}');
    Dir := ExpandConstant('{app}');
    if FileExists(Dir + '\{#StariLog}') and not FileExists(Dir + '\{#Log}') then
      RenameFile(Dir + '\{#StariLog}', Dir + '\{#Log}');
    if FileExists(Dir + '\{#StariLog}.1') and not FileExists(Dir + '\{#Log}.1') then
      RenameFile(Dir + '\{#StariLog}.1', Dir + '\{#Log}.1');
  end;
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
      DeleteFile(Dir + '\{#Log}');
      DeleteFile(Dir + '\{#Log}.1');
    end;
    RemoveDir(Dir);
  end;
end;
