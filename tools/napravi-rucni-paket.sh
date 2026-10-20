#!/usr/bin/env bash
# Pravi dist/TornadoKasa-<verzija>-rucno.zip: Windows exe (cross-build) +
# skripta koja radi isto što i Inno installer. Za Mac/Linux, gdje nema ISCC-a.
#   tools/napravi-rucni-paket.sh
# Treba: rustup target add x86_64-pc-windows-gnu && brew install mingw-w64
set -euo pipefail
cd "$(dirname "$0")/.."

verzija=$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)
cargo build --release --target x86_64-pc-windows-gnu

ime="TornadoKasa-$verzija-rucno"
paket="dist/$ime"
rm -rf "$paket" "$paket.zip"
mkdir -p "$paket"
cp target/x86_64-pc-windows-gnu/release/tornado-kasa.exe "$paket/"

# Windows PowerShell 5.1 čita .ps1 bez BOM-a kao ANSI i pokvari č/ć/š/ž/đ.
{ printf '\xEF\xBB\xBF'; sed "s/__VERZIJA__/$verzija/" installer/rucna-instalacija.ps1; } |
    sed 's/$/\r/' > "$paket/rucna-instalacija.ps1"
printf '@powershell -NoProfile -ExecutionPolicy Bypass -File "%%~dp0rucna-instalacija.ps1" %%*\r\n' > "$paket/instaliraj.cmd"
printf '@powershell -NoProfile -ExecutionPolicy Bypass -File "%%~dp0rucna-instalacija.ps1" -Deinstaliraj %%*\r\n' > "$paket/deinstaliraj.cmd"

(cd dist && zip -qr "$ime.zip" "$ime")
ls -l "$paket.zip"
