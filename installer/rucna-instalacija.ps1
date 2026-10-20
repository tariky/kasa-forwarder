# Ručna instalacija Tornado Kase, bez Inno Setupa. Radi isto što i
# installer\tornado-kasa.iss: po korisniku, bez admin prava, u
# %LOCALAPPDATA%\Programs\Tornado Kasa, pa se ta dva ne smiju razići.
#
# Pokreće se iz foldera u kojem je i tornado-kasa.exe (dvoklik na instaliraj.cmd):
#   instaliraj.cmd                          obična instalacija ili nadogradnja
#   instaliraj.cmd -Tiho                    bez pitanja, forwarder se pokrene u pozadini
#   instaliraj.cmd -BezAutostarta -PrecicaNaDesktopu
#   deinstaliraj.cmd [-Tiho]                isto kao Postavke -> Aplikacije -> Tornado Kasa
#
# Nadogradnja: pokreni noviji paket preko postojećeg. config.json i log ostaju.

param(
    [switch]$Deinstaliraj,
    [switch]$Tiho,
    [switch]$BezAutostarta,
    [switch]$PrecicaNaDesktopu
)

$ErrorActionPreference = 'Stop'

# Upisuje se pri pakovanju (tools/napravi-rucni-paket.sh), iz Cargo.toml.
$Verzija = '__VERZIJA__'
$AppIme = 'Tornado Kasa'
$ExeIme = 'tornado-kasa.exe'
$Dir = Join-Path $env:LOCALAPPDATA "Programs\$AppIme"
$RunKljuc = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
# Isto ime vrijednosti koristi i sam program (install/uninstall, prekidač u prozoru).
$RunVrijednost = 'TornadoKasa'
$UninstallKorijen = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
# Ključ koji pravi Inno Setup (AppId iz .iss). Ako postoji, instalaciju vodi on.
$InnoKljuc = "$UninstallKorijen\{8B4E2C1A-5F3D-4A7E-9C21-6D0F4B8A2E17}_is1"
$RucniKljuc = "$UninstallKorijen\TornadoKasa"
# Do 0.1.3 program se zvao Kasa Forwarder. Nadogradnja ostaje u starom folderu (kao Inno
# s istim AppId-jem), a stari exe, autostart, prečice i stavka za deinstalaciju se čiste.
$StariExe = 'kasa-forwarder.exe'
$StariRun = 'KasaForwarder'
$StariDir = Join-Path $env:LOCALAPPDATA 'Programs\Kasa Forwarder'
$StariRucniKljuc = "$UninstallKorijen\KasaForwarder"
if (-not (Test-Path $Dir) -and (Test-Path (Join-Path $StariDir $StariExe))) { $Dir = $StariDir }
$StarePrecice = @(
    (Join-Path ([Environment]::GetFolderPath('Programs')) 'Kasa Forwarder.lnk'),
    (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Kasa Forwarder.lnk')
)
$PrecicaStart = Join-Path ([Environment]::GetFolderPath('Programs')) "$AppIme.lnk"
$PrecicaDesktop = Join-Path ([Environment]::GetFolderPath('Desktop')) "$AppIme.lnk"

Add-Type -AssemblyName System.Windows.Forms

function Poruka([string]$tekst, [string]$ikona = 'Information') {
    if ($Tiho) { Write-Host $tekst; return }
    [void][System.Windows.Forms.MessageBox]::Show($tekst, $AppIme, 'OK', $ikona)
}

function Pitaj([string]$tekst) {
    # Default je Ne, kao MB_DEFBUTTON2 u installeru.
    [System.Windows.Forms.MessageBox]::Show($tekst, $AppIme, 'YesNo', 'Question', 'Button2') -eq 'Yes'
}

function ProcesRadi([string]$exe) {
    [bool](Get-Process -Name ([IO.Path]::GetFileNameWithoutExtension($exe)) -ErrorAction SilentlyContinue)
}

# Prvo uredno (WM_CLOSE — program skine ikonu iz traya i upiše STOP u log), pa na silu.
function Zatvori-Proces([string]$exe) {
    if (-not (ProcesRadi $exe)) { return }
    & taskkill.exe /IM $exe 2>&1 | Out-Null
    for ($i = 0; $i -lt 20; $i++) {
        if (-not (ProcesRadi $exe)) { return }
        Start-Sleep -Milliseconds 250
    }
    & taskkill.exe /F /IM $exe 2>&1 | Out-Null
    Start-Sleep -Milliseconds 500
}

function Zatvori-Forwarder {
    Zatvori-Proces $ExeIme
    Zatvori-Proces $StariExe
}

# Ostaci Kasa Forwardera: stari autostart bi pokretao obrisani exe, log ostaje pod novim imenom.
function Pocisti-StaroIme {
    Remove-ItemProperty -Path $RunKljuc -Name $StariRun -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $Dir $StariExe) -Force -ErrorAction SilentlyContinue
    Remove-Item $StarePrecice -Force -ErrorAction SilentlyContinue
    Remove-Item $StariRucniKljuc -Recurse -Force -ErrorAction SilentlyContinue
    foreach ($sufiks in '', '.1') {
        $stari = Join-Path $Dir "kasa-forwarder.log$sufiks"
        $novi = Join-Path $Dir "tornado-kasa.log$sufiks"
        if ((Test-Path $stari) -and -not (Test-Path $novi)) { Rename-Item $stari $novi }
    }
}

function Napravi-Precicu([string]$putanja, [string]$exe) {
    $s = (New-Object -ComObject WScript.Shell).CreateShortcut($putanja)
    $s.TargetPath = $exe
    $s.WorkingDirectory = Split-Path $exe
    $s.IconLocation = "$exe,0"
    $s.Description = "Log i podešavanja Tornado Kase"
    $s.Save()
}

function Instaliraj {
    $izvor = Join-Path $PSScriptRoot $ExeIme
    if (-not (Test-Path $izvor)) { throw "Nema $ExeIme pored skripte ($PSScriptRoot)." }
    $cilj = Join-Path $Dir $ExeIme

    Zatvori-Forwarder
    New-Item -ItemType Directory -Force -Path $Dir | Out-Null
    Pocisti-StaroIme
    if ((Resolve-Path $izvor).Path -ne [IO.Path]::GetFullPath($cilj)) {
        Copy-Item $izvor $cilj -Force
    }
    # Exe iz zipa skinutog s mreže nosi oznaku "s interneta" i SmartScreen bi pitao.
    Unblock-File $cilj -ErrorAction SilentlyContinue

    Napravi-Precicu $PrecicaStart $cilj
    if ($PrecicaNaDesktopu) { Napravi-Precicu $PrecicaDesktop $cilj }

    # Izbor ovdje ima prednost i nad prekidačem iz prozora forwardera, kao u installeru.
    if ($BezAutostarta) {
        Remove-ItemProperty -Path $RunKljuc -Name $RunVrijednost -ErrorAction SilentlyContinue
    } else {
        New-Item -Path $RunKljuc -Force | Out-Null
        Set-ItemProperty -Path $RunKljuc -Name $RunVrijednost -Value "`"$cilj`" --autostart"
    }

    # Stavka u Postavke -> Aplikacije. Ako je forwarder instaliran Inno Setupom,
    # njegova stavka (i unins000.exe) ostaju — ovo je onda samo nadogradnja exe-a.
    if (-not (Test-Path $InnoKljuc)) {
        $skripta = Join-Path $Dir 'rucna-instalacija.ps1'
        if ($PSCommandPath -ne $skripta) { Copy-Item $PSCommandPath $skripta -Force }
        $ps = "powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$skripta`" -Deinstaliraj"
        New-Item -Path $RucniKljuc -Force | Out-Null
        $vrijednosti = @{
            DisplayName          = $AppIme
            DisplayVersion       = $Verzija
            Publisher            = 'Tarik Caplja'
            DisplayIcon          = $cilj
            InstallLocation      = $Dir
            UninstallString      = $ps
            QuietUninstallString = "$ps -Tiho"
        }
        foreach ($k in $vrijednosti.Keys) { Set-ItemProperty -Path $RucniKljuc -Name $k -Value $vrijednosti[$k] }
        foreach ($k in 'NoModify', 'NoRepair') { Set-ItemProperty -Path $RucniKljuc -Name $k -Value 1 -Type DWord }
        Set-ItemProperty -Path $RucniKljuc -Name EstimatedSize -Value ([int]((Get-Item $cilj).Length / 1KB)) -Type DWord
    }

    # Obična instalacija otvara prozor forwardera, tiha ga pokreće u pozadini.
    if ($Tiho) {
        Start-Process -FilePath $cilj -ArgumentList '--autostart' -WorkingDirectory $Dir
        Write-Host "$AppIme $Verzija instaliran u $Dir"
    } else {
        Start-Process -FilePath $cilj -WorkingDirectory $Dir
    }
}

function Deinstaliraj {
    Zatvori-Forwarder
    # Autostart je možda uključen iz prozora forwardera, ne iz instalacije.
    Remove-ItemProperty -Path $RunKljuc -Name $RunVrijednost -ErrorAction SilentlyContinue
    Remove-Item $PrecicaStart, $PrecicaDesktop -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $Dir $ExeIme) -Force -ErrorAction SilentlyContinue
    Remove-Item $RucniKljuc -Recurse -Force -ErrorAction SilentlyContinue

    $pitanje = "Obrisati i podešavanja (config.json, token, adrese) i log?`n`nAko planiraš ponovo instalirati forwarder, odaberi Ne."
    if (-not $Tiho -and (Pitaj $pitanje)) {
        foreach ($f in 'config.json', 'tornado-kasa.log', 'tornado-kasa.log.1') {
            Remove-Item (Join-Path $Dir $f) -Force -ErrorAction SilentlyContinue
        }
    }
    Remove-Item (Join-Path $Dir 'rucna-instalacija.ps1') -Force -ErrorAction SilentlyContinue
    # Kao RemoveDir u installeru: folder nestaje samo ako je prazan.
    if ((Test-Path $Dir) -and -not (Get-ChildItem $Dir -Force)) { Remove-Item $Dir -Force }
    Poruka "$AppIme je deinstalirana."
}

try {
    if ($Deinstaliraj) { Deinstaliraj } else { Instaliraj }
} catch {
    Poruka "Nije uspjelo: $($_.Exception.Message)" 'Error'
    exit 1
}
