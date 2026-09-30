# Gradi release exe i installer: dist\KasaForwarder-Setup-<verzija>.exe
# Verzija se čita iz Cargo.toml. Traži Inno Setup 6.5+ ili 7 (ISCC.exe).
#   powershell -ExecutionPolicy Bypass -File tools\napravi-installer.ps1

$ErrorActionPreference = 'Stop'
$korijen = Split-Path $PSScriptRoot -Parent
Push-Location $korijen
try {
    $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
    if (-not (Test-Path $cargo)) { $cargo = 'cargo' }
    & $cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build nije uspio (je li forwarder pokrenut iz target\release? ugasi ga iz traya)" }

    $verzija = (Select-String -Path 'Cargo.toml' -Pattern '^version\s*=\s*"(.+)"').Matches[0].Groups[1].Value

    $kandidati = @(
        "$env:LOCALAPPDATA\Programs\Inno Setup 7\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 7\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 7\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
    )
    $iscc = $kandidati | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $iscc) { throw 'Inno Setup nije instaliran (https://jrsoftware.org/isdl.php)' }

    & $iscc "/DAppVersion=$verzija" 'installer\kasa-forwarder.iss'
    if ($LASTEXITCODE -ne 0) { throw 'ISCC nije uspio' }
    Get-Item "dist\KasaForwarder-Setup-$verzija.exe" | Select-Object Name, @{ n = 'MB'; e = { [math]::Round($_.Length / 1MB, 2) } }
}
finally {
    Pop-Location
}
